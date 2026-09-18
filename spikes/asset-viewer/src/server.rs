//! The local map-editor server behind `--serve`.
//!
//! This module is a **shell**. Every hard decision it makes -- which tileset a map is read through,
//! which tile a painted cell gets, whether a paint can be reproduced at all, what bytes a map
//! writes back as -- is made by [`crate::tile`] and [`crate::map`], which the CLI already drives.
//! What is new here is *duration*: the CLI parses, edits and exits, while a session holds one
//! parsed map across many paints. See [`EditorSession::map`].
//!
//! Two properties are deliberately structural rather than conventions the UI is trusted to keep:
//!
//! 1. **[`Editor::handle`] is a pure function of the request and the session.** The socket is in
//!    [`serve`] and nowhere else, so the endpoints are testable without a browser and without a
//!    port.
//! 2. **Refusals are values, not errors to hide.** A refused paint answers `200` with
//!    `"ok": false` and the library's own message, because a refusal -- "no tile of terrain 9
//!    accepts the neighbourhood at (4, 2)" -- is the most informative thing the tool can say. An
//!    HTTP error code would invite the client to swallow it as a transport failure.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt::Write as _;
use std::fs;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::map::MapAsset;
use crate::mpq::Archive;
use crate::paths::paths_are_same_file;
use crate::pbm::PbmImage;
use crate::png_export::write_rgba_png;
use crate::tile::{
    MapClass, TileSelector, TileSetDefinition, TileSetResolution, resolve_tileset, tileset_mismatch,
};

/// The page, its stylesheet and its script, compiled into the binary.
///
/// One file to distribute, and no way for a stale checkout of the frontend to disagree with the
/// server it is talking to.
const INDEX_HTML: &str = include_str!("ui/index.html");
const APP_JS: &str = include_str!("ui/app.js");
const STYLE_CSS: &str = include_str!("ui/style.css");

/// Where the editor gets a map's `.til` and its atlas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TileSetSource {
    /// Read the resolved tileset and its atlas straight out of `pic.mpq`, by member name.
    ///
    /// The member name comes from [`resolve_tileset`], so the archive form still never guesses --
    /// it only removes the manual extraction step the CLI requires. A map whose tileset does not
    /// resolve to exactly one member is refused here rather than defaulted.
    Archive(PathBuf),
    /// Loose files, the shape `--view-map` already takes.
    ///
    /// The supplied `.til` is checked against the gamescript binding by [`tileset_mismatch`], the
    /// same check `--map-paint-terrain` makes, so a modded tileset is accepted and a shipped one
    /// the engine would not use for this map is refused.
    Loose {
        definition: PathBuf,
        atlas: PathBuf,
    },
}

/// A tileset and atlas that have been loaded for one map, and the account of how they were chosen.
#[derive(Debug)]
pub struct LoadedTileSet {
    pub member: String,
    /// Why this tileset and not another, in the words the user is shown.
    pub provenance: String,
    pub definition: TileSetDefinition,
    pub atlas: PbmImage,
}

/// One open map, held across many paints.
pub struct EditorSession {
    pub path: PathBuf,
    /// The map as edited so far.
    ///
    /// **This is the usage the library has never had.** Every CLI verb parses, edits once and
    /// exits; here one `MapAsset` survives an unbounded number of paints before anything is
    /// written. `MapAsset::instance_id_high_water` is documented as a per-process counter that is
    /// worth keeping "for a caller that makes several edits against one parsed map". This is that
    /// caller.
    pub map: MapAsset,
    /// The states this session can go back to, oldest first, newest last.
    ///
    /// Bounded at [`UNDO_DEPTH`]. A whole-map snapshot of a 128x128 map is about 160 KB, so the
    /// whole stack is a few megabytes -- cheap enough not to need a diff, and bounded so a long
    /// session cannot grow without limit.
    undo: VecDeque<UndoStep>,
    pub tile_set: LoadedTileSet,
    /// The handle `/api/open` gave this session. See [`Editor::session_for`].
    token: String,
    /// **Which cells of the map as it stands now** hold a tile this tool drew among equally valid
    /// ones, keyed by packed cell index.
    ///
    /// A set of live cells rather than a running total, because a total is cumulative history and
    /// the question a save has to answer is about the *file*. Painting a region and then painting
    /// it back to something the tileset determines leaves a fully reproducible map, and the
    /// counter kept reporting the earlier draw. An honesty mechanism that over-reports gets
    /// ignored, which defeats it.
    drawn_cells: BTreeSet<usize>,
    /// Paints currently standing in the map. Decremented by undo, so it describes the map and not
    /// the session's history.
    paints: usize,
}

/// One step the session can be taken back to.
#[derive(Clone)]
struct UndoStep {
    map: MapAsset,
    drawn_cells: BTreeSet<usize>,
    paints: usize,
}

/// How many paints a session can walk back.
///
/// Bounded deliberately. Redo is not offered.
pub const UNDO_DEPTH: usize = 32;

/// A response, independent of any HTTP library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

impl HttpResponse {
    fn json(body: String) -> Self {
        Self {
            status: 200,
            content_type: "application/json; charset=utf-8",
            body: body.into_bytes(),
        }
    }

    /// A refusal the user is meant to read.
    ///
    /// Status `200`, deliberately: this is an answer, not a transport failure. See the module note.
    fn refusal(reason: &str, notes: &[String]) -> Self {
        Self::json(format!(
            "{{\"ok\":false,\"refusal\":{},\"notes\":{}}}",
            json_string(reason),
            json_strings(notes)
        ))
    }

    fn text(status: u16, content_type: &'static str, body: &str) -> Self {
        Self {
            status,
            content_type,
            body: body.as_bytes().to_vec(),
        }
    }
}

/// One request, reduced to what the router needs.
///
/// `host` and `origin` are carried into the pure layer deliberately. They are not transport
/// details: they are the only thing separating this editor from any web page the user happens to
/// have open, and a guard that lives in the socket loop is a guard the handler tests cannot see.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HttpRequest<'a> {
    pub method: &'a str,
    /// The raw request target: path and query together.
    pub target: &'a str,
    pub body: &'a str,
    pub host: Option<&'a str>,
    pub origin: Option<&'a str>,
}

/// What kind of path a native dialog is being asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickKind {
    /// A folder, for the maps directory.
    Directory,
    /// A file to create, for Save As.
    SaveFile,
}

/// What the page is asking the operating system for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickRequest {
    pub kind: PickKind,
    /// Where the dialog should start, when it exists.
    pub start_in: Option<PathBuf>,
    pub default_name: Option<String>,
}

/// What a native dialog answered.
///
/// **Cancelling is not an error.** A user who dismisses a dialog has told the tool something
/// perfectly ordinary, and turning that into a refusal in the log trains people to ignore the log
/// -- which is the one place this editor says things that matter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickOutcome {
    Chosen(PathBuf),
    Cancelled,
    /// No dialog could be shown, with the reason to put in front of the user.
    Unavailable(String),
}

/// How the editor asks the operating system for a path.
///
/// A seam, because a file dialog cannot run in CI: a test substitutes one that answers
/// immediately. Everything downstream of it -- the endpoint, the validation the chosen path then
/// goes through, the cancel path and the unavailable path -- is covered without a desktop.
pub type Picker = Box<dyn Fn(&PickRequest) -> PickOutcome + Send>;

/// How long a dialog may stay up before the editor gives up on it.
///
/// **The request loop is single-threaded, so a blocked picker freezes the whole editor.** A user
/// standing in front of the dialog is not using the editor anyway, so the bound is generous; what
/// it rules out is the case where no dialog ever appeared and the child waits forever.
pub const PICK_TIMEOUT: Duration = Duration::from_secs(120);

/// The whole editor: a tileset source, and at most one open map.
pub struct Editor {
    source: TileSetSource,
    session: Option<EditorSession>,
    /// The port the listener is bound to.
    ///
    /// Held because the `Host` and `Origin` checks are against *this* editor's address. A check
    /// against "any loopback port" would let one local server's page drive another's.
    port: u16,
    counter: u64,
    /// How to ask the operating system for a path. See [`Picker`].
    picker: Picker,
}

impl Editor {
    pub fn new(source: TileSetSource, port: u16) -> Self {
        Self {
            source,
            session: None,
            port,
            counter: 0,
            picker: Box::new(native_pick),
        }
    }

    /// Replace the native dialog with something else. For tests.
    pub fn set_picker(&mut self, picker: Picker) {
        self.picker = picker;
    }

    /// The open session, for tests and for [`serve`]'s startup banner.
    pub fn session(&self) -> Option<&EditorSession> {
        self.session.as_ref()
    }

    /// Route one request.
    ///
    /// **Every request passes the browser guard first**, including the static assets. Loopback is
    /// not an origin boundary: a page on any site can reach `127.0.0.1`, and before this check
    /// `<img src="http://127.0.0.1:8731/api/open?path=...">` was enough to swap the held map, a
    /// form POST was enough to paint into it, and `/api/save` was enough to write a 159 KB file
    /// anywhere the user can write. None of that needed to read a response, so CORS never applied.
    pub fn handle(&mut self, request: &HttpRequest<'_>) -> HttpResponse {
        if let Some(reason) = cross_origin_refusal(request, self.port) {
            // 403, not 200: this one is not an answer the user asked for, and no page of ours can
            // provoke it. A client that sees it has been driven by something else.
            return HttpResponse {
                status: 403,
                content_type: "application/json; charset=utf-8",
                body: format!(
                    "{{\"ok\":false,\"refusal\":{},\"notes\":[]}}",
                    json_string(&reason)
                )
                .into_bytes(),
            };
        }
        let (path, query) = match request.target.split_once('?') {
            Some((path, query)) => (path, query),
            None => (request.target, ""),
        };
        match (request.method, path) {
            ("GET", "/") | ("GET", "/index.html") => {
                HttpResponse::text(200, "text/html; charset=utf-8", INDEX_HTML)
            }
            ("GET", "/app.js") => {
                HttpResponse::text(200, "text/javascript; charset=utf-8", APP_JS)
            }
            ("GET", "/style.css") => HttpResponse::text(200, "text/css; charset=utf-8", STYLE_CSS),
            // **POST, not GET.** Opening a map replaces the server's whole session, and a
            // state-mutating GET is reachable from a bare `<img>` tag on any page in the world.
            ("GET", "/api/config") => self.config(),
            ("GET", "/api/list") => list_directory(&form_fields(query)),
            // **POST, and guarded like everything else.** A cross-origin page must not be able to
            // make the user's machine pop a file dialog. No session handle is required, because
            // choosing the maps directory happens before any map is open and the endpoint touches
            // no session state.
            ("POST", "/api/pick-directory") => self.pick(PickKind::Directory, &form_fields(request.body)),
            ("POST", "/api/pick-save") => self.pick(PickKind::SaveFile, &form_fields(request.body)),
            ("POST", "/api/open") => self.open(&form_fields(request.body)),
            ("GET", "/api/atlas.png") => self.atlas_png(&form_fields(query)),
            ("POST", "/api/paint") => self.paint(&form_fields(request.body)),
            ("POST", "/api/undo") => self.undo(&form_fields(request.body)),
            ("POST", "/api/save") => self.save(&form_fields(request.body)),
            _ => HttpResponse {
                status: 404,
                content_type: "application/json; charset=utf-8",
                body: format!(
                    "{{\"ok\":false,\"refusal\":{},\"notes\":[]}}",
                    json_string(&format!("no endpoint {} {path}", request.method))
                )
                .into_bytes(),
            },
        }
    }

    /// The open session, if the caller's handle names it.
    ///
    /// **One `Editor` holds one map, so a second browser tab is a real hazard rather than a
    /// theoretical one**: tab A opens X, tab B opens Y, and tab A's next paint lands in Y at A's
    /// coordinates while A's canvas goes on showing X. The handle does not make two maps possible;
    /// it makes the stale tab fail loudly instead of silently editing the wrong file. It is a
    /// handle, not a secret -- the `Host` and `Origin` checks are what keep other sites out.
    fn session_for(&mut self, fields: &BTreeMap<String, String>) -> Result<&mut EditorSession, HttpResponse> {
        let supplied = fields.get("token").map(String::as_str).unwrap_or_default();
        match self.session.as_ref() {
            None => Err(HttpResponse::refusal("no map is open", &[])),
            Some(session) if session.token != supplied => Err(HttpResponse::refusal(
                &format!(
                    "this tab is holding a handle to a map that is no longer the open one. The \
                     editor holds a single map per process, and {} is what it has now. Reload this \
                     page to take it over",
                    session.path.display()
                ),
                &[],
            )),
            Some(_) => Ok(self.session.as_mut().expect("just matched Some")),
        }
    }

    /// Open a map, or refuse **without disturbing the map already held**.
    ///
    /// A failed open used to leave the client saying "No map open." while the server went on
    /// holding the previous session, and Save As was not gated on the client's belief -- so a typo
    /// in the path, followed by a save, wrote a file the user had been told did not exist. Dropping
    /// the session instead would be worse: a typo would destroy an unsaved session outright. So the
    /// session survives and the refusal **names what is still held**, and the client says so.
    /// What the page can fill in for the user without being told.
    ///
    /// In a standard install the maps sit in `map/` beside `pic.mpq`, so `--pic` already names the
    /// directory. Suggested only when it exists, and only as a default in a field the user can
    /// change -- nothing here decides what is opened.
    fn config(&self) -> HttpResponse {
        let suggestion = match &self.source {
            TileSetSource::Archive(archive) => archive
                .parent()
                .map(|parent| parent.join("map"))
                .filter(|directory| directory.is_dir()),
            TileSetSource::Loose { .. } => None,
        };
        HttpResponse::json(format!(
            "{{\"ok\":true,\"mapsDirectory\":{},\"undoDepth\":{UNDO_DEPTH},\"notes\":[]}}",
            suggestion.map_or_else(
                || "null".to_owned(),
                |directory| json_string(&directory.display().to_string())
            ),
        ))
    }

    /// Ask the operating system for a path.
    ///
    /// **The path that comes back is trusted exactly as far as a typed one, which is to say not at
    /// all.** It goes through the same listing confinement and the same save guards; the dialog is
    /// an accelerator for the field above it and never a second route into the filesystem.
    fn pick(&mut self, kind: PickKind, fields: &BTreeMap<String, String>) -> HttpResponse {
        let start_in = fields
            .get("dir")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .filter(|directory| directory.is_dir());
        let request = PickRequest {
            kind,
            start_in,
            default_name: fields
                .get("name")
                .filter(|value| !value.is_empty())
                .cloned(),
        };
        match (self.picker)(&request) {
            PickOutcome::Chosen(path) => HttpResponse::json(format!(
                "{{\"ok\":true,\"cancelled\":false,\"path\":{},\"notes\":[]}}",
                json_string(&path.display().to_string())
            )),
            // Not a refusal: dismissing a dialog is an ordinary thing to do, and a log full of
            // non-events is a log nobody reads.
            PickOutcome::Cancelled => HttpResponse::json(
                "{\"ok\":true,\"cancelled\":true,\"path\":null,\"notes\":[]}".to_owned(),
            ),
            PickOutcome::Unavailable(reason) => HttpResponse::refusal(
                &format!("{reason}. Type the path into the field instead -- it does the same thing"),
                &[],
            ),
        }
    }

    fn open(&mut self, fields: &BTreeMap<String, String>) -> HttpResponse {
        let mut notes = Vec::new();
        match self.open_map(fields, &mut notes) {
            Ok(body) => HttpResponse::json(body),
            Err(reason) => self.open_refusal(&reason, &notes),
        }
    }

    /// A refusal from `/api/open`, carrying the path this editor is still holding.
    fn open_refusal(&self, reason: &str, notes: &[String]) -> HttpResponse {
        let holding = self.session.as_ref().map_or_else(
            || "null".to_owned(),
            |session| json_string(&session.path.display().to_string()),
        );
        HttpResponse::json(format!(
            "{{\"ok\":false,\"refusal\":{},\"holding\":{holding},\"notes\":{}}}",
            json_string(reason),
            json_strings(notes)
        ))
    }

    fn open_map(
        &mut self,
        fields: &BTreeMap<String, String>,
        notes: &mut Vec<String>,
    ) -> Result<String, String> {
        let Some(path) = fields.get("path").filter(|value| !value.is_empty()) else {
            return Err("no map path was given".to_owned());
        };
        let path = PathBuf::from(path);
        let bytes = fs::read(&path)
            .map_err(|error| format!("could not read {}: {error}", path.display()))?;
        let map = MapAsset::parse(&bytes)
            .map_err(|error| format!("{} does not parse as a map: {error}", path.display()))?;
        let class = MapClass::from_path(&path).ok_or_else(|| {
            format!(
                "{} parses as a map but its extension is not one the corpus classifies, so which \
                 tileset the engine reads it through is unknown; the known classes are .smp \
                 (combat) and .scn/.lgd/.map (world)",
                path.display()
            )
        })?;
        let tile_set = load_tile_set(&path, &self.source, notes)?;

        self.counter += 1;
        let token = mint_token(self.counter);
        let body = open_json(&path, class, &map, &tile_set, &token, notes);
        self.session = Some(EditorSession {
            path,
            map,
            undo: VecDeque::new(),
            tile_set,
            token,
            drawn_cells: BTreeSet::new(),
            paints: 0,
        });
        Ok(body)
    }

    fn atlas_png(&mut self, fields: &BTreeMap<String, String>) -> HttpResponse {
        let session = match self.session_for(fields) {
            Ok(session) => session,
            Err(response) => return response,
        };
        let atlas = &session.tile_set.atlas;
        let mut png = Vec::new();
        match write_rgba_png(&mut png, atlas.width, atlas.height, &atlas.rgba) {
            Ok(()) => HttpResponse {
                status: 200,
                content_type: "image/png",
                body: png,
            },
            Err(error) => HttpResponse::refusal(
                &format!("could not encode the tile atlas: {error}"),
                &[],
            ),
        }
    }

    fn paint(&mut self, fields: &BTreeMap<String, String>) -> HttpResponse {
        let session = match self.session_for(fields) {
            Ok(session) => session,
            Err(response) => return response,
        };
        let numbers = ["x0", "y0", "x1", "y1", "terrain"]
            .iter()
            .map(|name| {
                fields
                    .get(*name)
                    .ok_or_else(|| format!("paint needs a {name}"))
                    .and_then(|value| {
                        value
                            .parse::<u32>()
                            .map_err(|_| format!("{name} must be a nonnegative integer: {value}"))
                    })
            })
            .collect::<Result<Vec<u32>, String>>();
        let numbers = match numbers {
            Ok(numbers) => numbers,
            Err(reason) => return HttpResponse::refusal(&reason, &[]),
        };
        let selector = match fields.get("seed") {
            None => TileSelector::LowestSlot,
            Some(value) => match value.parse::<u64>() {
                Ok(seed) => TileSelector::Seeded(seed),
                Err(_) => {
                    return HttpResponse::refusal(
                        &format!("seed must be a nonnegative integer: {value}"),
                        &[],
                    );
                }
            },
        };
        let rect = (numbers[0], numbers[1], numbers[2], numbers[3]);
        let terrain_type = numbers[4];

        // The snapshot is taken first and put back on any error. Every refusal reachable today
        // comes out of `plan_terrain_paint`, which `paint_terrain` runs before it writes a single
        // cell, so nothing known gets as far as needing the restore -- it is there for an error
        // raised mid-apply, which no input is known to produce. An earlier version also planned
        // the paint separately here; that plan's only consumer was a `debug_assert_eq!`, so
        // release builds ran a full second constraint solve of a whole-map rectangle for nothing.
        let before = session.map.clone();
        let paint = match session.map.paint_terrain(
            rect,
            terrain_type,
            &session.tile_set.definition,
            selector,
        ) {
            Ok(paint) => paint,
            Err(error) => {
                session.map = before;
                return HttpResponse::refusal(&error.to_string(), &[]);
            }
        };
        let width = session.map.width;
        let mut notes = Vec::new();
        let drawn = paint.plan.drawn_cells();
        let written = paint.plan.region.len() + paint.plan.ring.len();
        if drawn > 0 {
            notes.push(format!(
                "{drawn} of the {written} written cells were newly painted with several equally \
                 valid tiles and none already in place. The engine draws among those at random -- \
                 the same paint run twice gave centre tiles 385 and 390 -- so they are a legal \
                 choice, not the engine's. Set a seed for a different legal draw."
            ));
        }
        let edge = paint.plan.cells_touching_a_map_edge();
        if edge > 0 {
            notes.push(format!(
                "{edge} written cells have a neighbour off the map. This writer treats an off-map \
                 neighbour as satisfying any constraint, which no saved artifact tests."
            ));
        }
        // Per cell, and both ways: a cell the tileset now determines stops being a draw. Painting
        // over an earlier draw is the ordinary case and it used to leave the account overstated
        // for the rest of the session.
        session.undo.push_back(UndoStep {
            map: before,
            drawn_cells: session.drawn_cells.clone(),
            paints: session.paints,
        });
        while session.undo.len() > UNDO_DEPTH {
            session.undo.pop_front();
        }
        for cell in paint.plan.cells() {
            let index = (cell.y * width + cell.x) as usize;
            if cell.choice.is_reproducible() {
                session.drawn_cells.remove(&index);
            } else {
                session.drawn_cells.insert(index);
            }
        }
        session.paints += 1;

        let mut cells = String::from("[");
        for (position, cell) in paint.plan.cells().enumerate() {
            if position > 0 {
                cells.push(',');
            }
            let index = cell.y * session.map.width + cell.x;
            let _ = write!(cells, "{{\"i\":{index},\"tile\":{}}}", cell.tile_index);
        }
        cells.push(']');
        let (x0, y0, x1, y1) = rect;
        let summary = format!(
            "paint ({x0}, {y0})..({x1}, {y1}) terrain:{terrain_type} region:{} ring:{} \
             cells-changed:{} drawn:{drawn}",
            paint.plan.region.len(),
            paint.plan.ring.len(),
            paint.cells_changed,
        );
        HttpResponse::json(format!(
            "{{\"ok\":true,\"cells\":{cells},\"summary\":{},\"undoDepth\":{},\"notes\":{}}}",
            json_string(&summary),
            session.undo.len(),
            json_strings(&notes)
        ))
    }

    fn undo(&mut self, fields: &BTreeMap<String, String>) -> HttpResponse {
        let session = match self.session_for(fields) {
            Ok(session) => session,
            Err(response) => return response,
        };
        let Some(previous) = session.undo.pop_back() else {
            return HttpResponse::refusal(
                &format!(
                    "there is nothing left to undo: this session keeps the last {UNDO_DEPTH} \
                     paints, and every one of them has been taken back"
                ),
                &[],
            );
        };
        // The map and its draw account move together. They used to be tracked apart, and the
        // account was reset to zero on undo -- which made a 197-paint session report a clean file
        // after taking back the last one.
        session.map = previous.map;
        session.drawn_cells = previous.drawn_cells;
        session.paints = previous.paints;
        HttpResponse::json(format!(
            "{{\"ok\":true,\"tiles\":{},\"undoDepth\":{},\"notes\":[]}}",
            tiles_json(&session.map),
            session.undo.len(),
        ))
    }

    fn save(&mut self, fields: &BTreeMap<String, String>) -> HttpResponse {
        let session = match self.session_for(fields) {
            Ok(session) => session,
            Err(response) => return response,
        };
        let target = match target_path(fields) {
            Ok(target) => target,
            Err(reason) => return HttpResponse::refusal(&reason, &[]),
        };
        match save_session(session, &target) {
            Ok(bytes) => {
                let mut notes = Vec::new();
                if !session.drawn_cells.is_empty() {
                    notes.push(format!(
                        "{} {} a tile drawn among equally valid ones, across {} {}. They are a \
                         legal choice, not the engine's: the engine draws at random there and \
                         that draw cannot be reproduced.",
                        session.drawn_cells.len(),
                        if session.drawn_cells.len() == 1 {
                            "cell in this file still holds"
                        } else {
                            "cells in this file still hold"
                        },
                        session.paints,
                        if session.paints == 1 { "paint" } else { "paints" },
                    ));
                }
                HttpResponse::json(format!(
                    "{{\"ok\":true,\"path\":{},\"bytes\":{bytes},\"notes\":{}}}",
                    json_string(&target.display().to_string()),
                    json_strings(&notes)
                ))
            }
            Err(reason) => HttpResponse::refusal(&reason, &[]),
        }
    }
}

/// Write the session's map to a path that does not exist yet.
///
/// **Never in place.** The loose `map/` directory has no backup, so this repeats every guard the
/// CLI's `edit_map` makes and adds nothing of its own: the same-file check by device and inode, the
/// map-class check on the output extension, a re-parse of the encoded bytes before they reach disk,
/// `create_new`, and removal of a short write.
fn save_session(session: &EditorSession, target: &Path) -> Result<usize, String> {
    if paths_are_same_file(&session.path, target) {
        return Err(format!(
            "refusing to write to the open map {}; save to a different path",
            session.path.display()
        ));
    }
    // The output path decides what the saved map is loaded *as*, so a `.scn` must not be saved as
    // a `.smp`: the two are read through different tilesets, and the art would be wrong.
    let source_class = MapClass::from_path(&session.path);
    let target_class = MapClass::from_path(target);
    if source_class != target_class {
        return Err(format!(
            "refusing to save {} as {}: {} and {} are read through different tilesets, so writing \
             one under the other's extension produces a map the game will draw with the wrong art. \
             Use a matching extension",
            session.path.display(),
            target.display(),
            describe_class(source_class),
            describe_class(target_class),
        ));
    }
    let encoded = session.map.to_bytes().map_err(|error| error.to_string())?;
    let reparsed = MapAsset::parse(&encoded)
        .map_err(|error| format!("refusing to write: the edited map no longer parses: {error}"))?;
    if reparsed.cells.len() != session.map.cells.len()
        || reparsed
            .cells
            .iter()
            .zip(&session.map.cells)
            .any(|(written, held)| !written.has_same_bytes(held))
    {
        return Err(
            "refusing to write: the encoded map does not read back with the cells that were \
             painted"
                .to_owned(),
        );
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                // The native save dialog asks its own "replace?" question and will hand back an
                // existing path if the user says yes. **We refuse it anyway.** The loose `map/`
                // directory has no backup, never writing in place is this tool's rule, and an OS
                // dialog does not get to override it -- but the user has just confirmed a replace,
                // so the refusal has to say why it did not happen.
                format!(
                    "{} already exists. This editor never writes over an existing file, even one \
                     you confirmed replacing in the save dialog: the map directory has no backup. \
                     Choose a name that is not taken",
                    target.display()
                )
            } else {
                format!("could not create {}: {error}", target.display())
            }
        })?;
    if let Err(error) = file.write_all(&encoded) {
        drop(file);
        let _ = fs::remove_file(target);
        return Err(format!("could not write {}: {error}", target.display()));
    }
    Ok(encoded.len())
}

/// The map files in one directory.
///
/// **Not a filesystem browser.** It lists the files of the directory it is given and nothing else:
/// no subdirectories are descended, no parent is reported, and nothing recurses. The directory
/// itself is whatever the user typed, which is the same trust `/api/open` already extends to a
/// path -- what this must not become is a way to walk the disk from a place the user chose.
///
/// The directory is **not canonicalised**, because the obvious workaround for the long-path
/// problem is a symlink -- `/tmp/lommaps` pointing into the install -- and resolving it would make
/// the listed names disagree with the path the user is working in.
fn list_directory(fields: &BTreeMap<String, String>) -> HttpResponse {
    let Some(directory) = fields.get("dir").filter(|value| !value.is_empty()) else {
        return HttpResponse::refusal("no directory was given", &[]);
    };
    let directory = PathBuf::from(directory);
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) => {
            return HttpResponse::refusal(
                &format!("could not list {}: {error}", directory.display()),
                &[],
            );
        }
    };
    let mut names: Vec<(String, u64)> = Vec::new();
    let mut skipped = 0_usize;
    for entry in entries.flatten() {
        let path = entry.path();
        // Files only, and only the extensions the corpus classifies. A directory in the listing
        // would invite descending into it, which is the line this endpoint does not cross.
        if !path.is_file() {
            continue;
        }
        if path
            .extension()
            .and_then(|extension| extension.to_str())
            .and_then(MapClass::from_extension)
            .is_none()
        {
            skipped += 1;
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let size = entry.metadata().map(|metadata| metadata.len()).unwrap_or(0);
        names.push((name, size));
    }
    names.sort_by_key(|(name, _)| name.to_lowercase());

    let mut listing = String::from("[");
    for (position, (name, size)) in names.iter().enumerate() {
        if position > 0 {
            listing.push(',');
        }
        let _ = write!(listing, "{{\"name\":{},\"size\":{size}}}", json_string(name));
    }
    listing.push(']');
    let mut notes = Vec::new();
    if skipped > 0 {
        notes.push(format!(
            "{skipped} other {} in this directory {} not a map by extension and {} not listed",
            if skipped == 1 { "file" } else { "files" },
            if skipped == 1 { "is" } else { "are" },
            if skipped == 1 { "is" } else { "are" },
        ));
    }
    HttpResponse::json(format!(
        "{{\"ok\":true,\"dir\":{},\"entries\":{listing},\"notes\":{}}}",
        json_string(&directory.display().to_string()),
        json_strings(&notes)
    ))
}

/// Where a save should write: either a whole path, or a directory plus one filename.
///
/// The two-field form is what the page uses, so the user types a filename and not 180 characters
/// of absolute path. **`name` must be a single ordinary component.** Joining a typed
/// `../../../etc/passwd` onto a chosen directory is a surprise even though `create_new` would
/// still refuse an existing file -- the user asked to write in *this* directory, and the tool
/// should write there or refuse.
fn target_path(fields: &BTreeMap<String, String>) -> Result<PathBuf, String> {
    if let Some(path) = fields.get("path").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    let directory = fields
        .get("dir")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "no output path was given".to_owned())?;
    let name = fields
        .get("name")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "no output filename was given".to_owned())?;
    if !is_single_component(name) {
        return Err(format!(
            "{name} is not a plain filename. Saving writes into the directory you chose, so the \
             name may not contain a path separator or a parent reference"
        ));
    }
    Ok(PathBuf::from(directory).join(name))
}

/// Whether `name` is exactly one ordinary path component.
fn is_single_component(name: &str) -> bool {
    let mut components = Path::new(name).components();
    matches!(components.next(), Some(std::path::Component::Normal(_))) && components.next().is_none()
}

/// AppleScript's error number for a dialog the user dismissed.
///
/// Matched on the number rather than on "User canceled", because the text is localised and the
/// number is not. A cancel misread as a failure puts a refusal in front of someone who did nothing
/// wrong.
const APPLESCRIPT_USER_CANCELLED: &str = "-128";

/// The folder chooser, as a script that takes its prompt from `argv`.
///
/// **The prompt is an argument, never interpolated into the script text**, and there is no `sh -c`
/// anywhere in this file. The strings here are the user's own, so this is hygiene rather than a
/// live threat -- but a quoting bug in a path like `Program Files (x86)` is exactly the class of
/// problem this whole feature exists to remove.
///
/// Arguments follow the script with **no `-` separator**. The `-` form is for reading a script
/// from standard input; after `-e` it is not consumed and arrives as `item 1 of argv`, which put
/// the prompt in item 2 and the directory in item 3. Measured against `osascript` on 2026-09-17,
/// and every string would have been off by one.
const CHOOSE_FOLDER: &str = r#"on run argv
    set chosen to choose folder with prompt (item 1 of argv)
    return POSIX path of chosen
end run"#;

const CHOOSE_FOLDER_IN: &str = r#"on run argv
    set chosen to choose folder with prompt (item 1 of argv) default location (POSIX file (item 2 of argv))
    return POSIX path of chosen
end run"#;

const CHOOSE_SAVE_NAME: &str = r#"on run argv
    set chosen to choose file name with prompt (item 1 of argv) default name (item 2 of argv)
    return POSIX path of chosen
end run"#;

const CHOOSE_SAVE_NAME_IN: &str = r#"on run argv
    set chosen to choose file name with prompt (item 1 of argv) default name (item 2 of argv) default location (POSIX file (item 3 of argv))
    return POSIX path of chosen
end run"#;

/// Ask macOS for a path through `osascript`.
///
/// `osascript` is in the base system, so this needs no dependency. **Windows and Linux have no
/// equivalent one-liner and are not covered**; they need a crate such as `rfd`, and that is the
/// packaging gap. On those platforms this reports itself unavailable and the typed field -- which
/// is not going away and which every test drives -- keeps working.
#[cfg(target_os = "macos")]
fn native_pick(request: &PickRequest) -> PickOutcome {
    run_picker(pick_command(request), PICK_TIMEOUT)
}

/// The `osascript` invocation for one request.
///
/// Split out so the argument list can be asserted without a desktop. That is not ceremony: the
/// first version put a `-` between the script and its arguments, which shifted every string by one
/// and would have shown a dialog prompted `-`.
#[cfg(target_os = "macos")]
fn pick_command(request: &PickRequest) -> Command {
    let start_in = request
        .start_in
        .as_ref()
        .map(|directory| directory.display().to_string());
    let mut command = Command::new("osascript");
    match request.kind {
        PickKind::Directory => {
            let prompt = "Choose the directory your maps are in";
            match &start_in {
                Some(directory) => command.args(["-e", CHOOSE_FOLDER_IN, prompt, directory]),
                None => command.args(["-e", CHOOSE_FOLDER, prompt]),
            };
        }
        PickKind::SaveFile => {
            let prompt = "Save the edited map as a new file";
            let name = request.default_name.as_deref().unwrap_or("edited.scn");
            match &start_in {
                Some(directory) => command.args(["-e", CHOOSE_SAVE_NAME_IN, prompt, name, directory]),
                None => command.args(["-e", CHOOSE_SAVE_NAME, prompt, name]),
            };
        }
    }
    command
}

#[cfg(not(target_os = "macos"))]
fn native_pick(_request: &PickRequest) -> PickOutcome {
    PickOutcome::Unavailable(
        "this build has no native file dialog: only macOS is covered, through osascript".to_owned(),
    )
}

/// Run a dialog process, and **kill it rather than wait forever**.
///
/// A dialog that never appears -- no window server, no automation permission, a headless session --
/// leaves a child blocked on a window that will never be drawn. The request loop is single-threaded,
/// so that child freezes the editor. Polling with a deadline bounds it; a hang is the one outcome
/// there is no recovering from.
fn run_picker(mut command: Command, timeout: Duration) -> PickOutcome {
    let mut child = match command.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() {
        Ok(child) => child,
        Err(error) => {
            return PickOutcome::Unavailable(format!(
                "could not start the file dialog: {error}"
            ));
        }
    };
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return PickOutcome::Unavailable(format!(
                        "the file dialog did not answer within {} seconds and was stopped; there \
                         may be no desktop session to show it on",
                        timeout.as_secs().max(1)
                    ));
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(error) => {
                return PickOutcome::Unavailable(format!("the file dialog failed: {error}"));
            }
        }
    }
    // The child has exited, so the pipes hold everything it wrote and this cannot block. The
    // output is one path, far short of a pipe buffer.
    match child.wait_with_output() {
        Ok(output) => classify_pick(
            output.status.success(),
            &output.stdout,
            &String::from_utf8_lossy(&output.stderr),
        ),
        Err(error) => PickOutcome::Unavailable(format!("the file dialog failed: {error}")),
    }
}

/// Turn a finished dialog process into an outcome.
///
/// Pure, so the three cases that matter -- a path, a cancel, a failure -- are testable without a
/// desktop. Separating cancel from failure is the whole point: `osascript` exits non-zero for both.
fn classify_pick(success: bool, stdout: &[u8], stderr: &str) -> PickOutcome {
    if success {
        let path = String::from_utf8_lossy(stdout).trim().to_owned();
        if path.is_empty() {
            return PickOutcome::Unavailable(
                "the file dialog returned no path at all".to_owned(),
            );
        }
        return PickOutcome::Chosen(PathBuf::from(path));
    }
    if stderr.contains(APPLESCRIPT_USER_CANCELLED) {
        return PickOutcome::Cancelled;
    }
    let reason = stderr.trim();
    PickOutcome::Unavailable(format!(
        "the file dialog could not be shown{}",
        if reason.is_empty() {
            String::new()
        } else {
            format!(": {reason}")
        }
    ))
}

/// Why a request must not be served, or `None` when it may proceed.
///
/// **Loopback is not an origin boundary.** Any page on any site can issue requests to
/// `127.0.0.1`, and a request that only *writes* never needs to read the response, so the
/// same-origin policy and CORS do not stop it. Two headers do:
///
/// - **`Origin`** identifies the page that caused the request. Browsers send it on every
///   cross-origin fetch and on every form POST. It is absent on a same-origin navigation and on
///   the plain `<img>`/`<script>`/`<link>` loads that made a state-mutating GET reachable, so it
///   is checked when present and cannot be relied on alone.
/// - **`Host`** is the authority the browser *thinks* it is talking to, and it is what closes DNS
///   rebinding. An attacker who points `evil.example` at `127.0.0.1` gets a browser that treats
///   the responses as same-origin and can read them -- but it sends `Host: evil.example:PORT`,
///   which is not a loopback literal. Requiring one is the whole defence; `Origin` alone would not
///   see this attack at all, because after rebinding the request *is* same-origin.
///
/// Both are checked against **this** editor's port, not "any loopback port", so one local server's
/// page cannot drive another's. A missing `Host` is refused: HTTP/1.1 requires it, and a request
/// without one is not a browser's.
fn cross_origin_refusal(request: &HttpRequest<'_>, port: u16) -> Option<String> {
    let Some(host) = request.host else {
        return Some(
            "refusing a request with no Host header. This editor reads and writes local files and \
             only answers its own page"
                .to_owned(),
        );
    };
    if !is_loopback_authority(host, port) {
        return Some(format!(
            "refusing a request for host {host}: this editor only answers to a loopback address on \
             port {port}. A name that resolves to 127.0.0.1 is not the same thing -- that is how a \
             web page reaches a local server it was never meant to see"
        ));
    }
    if let Some(origin) = request.origin
        && !is_own_origin(origin, port)
    {
        return Some(format!(
            "refusing a request from origin {origin}: this editor reads and writes local files on \
             request and only answers its own page"
        ));
    }
    None
}

/// Whether an `Origin` is this editor's own page.
fn is_own_origin(origin: &str, port: u16) -> bool {
    origin
        .strip_prefix("http://")
        .is_some_and(|authority| is_loopback_authority(authority, port))
}

/// Whether `authority` is a loopback literal carrying this editor's port.
///
/// `localhost` is included because a browser cannot be made to resolve it elsewhere, so a request
/// carrying it really did come from a page the user typed. Any other name is refused however it
/// resolves -- the resolution is exactly what an attacker controls.
fn is_loopback_authority(authority: &str, port: u16) -> bool {
    // `[::1]:8731` has colons inside the brackets, so the port is whatever follows the last one,
    // and only when it is not inside them.
    let (name, supplied_port) = match authority.rsplit_once(':') {
        Some((name, tail)) if !tail.contains(']') => (name, Some(tail)),
        _ => (authority, None),
    };
    let loopback = matches!(name, "127.0.0.1" | "localhost" | "[::1]" | "::1");
    match supplied_port {
        Some(supplied) => loopback && supplied.parse::<u16>() == Ok(port),
        // No port means 80. Refusing that outright would be wrong when the editor really is on 80.
        None => loopback && port == 80,
    }
}

/// A per-session handle.
///
/// Not a secret and not presented as one -- `Host` and `Origin` are what keep other sites out.
/// This exists so a browser tab holding a stale handle fails loudly instead of painting into a map
/// it is not showing. The clock is mixed in so two runs of the editor do not hand out the same
/// first handle, which would let a reloaded page silently adopt a new session.
fn mint_token(counter: u64) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos() as u64);
    let mut state = nanos ^ counter.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    state ^= state >> 33;
    state = state.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    state ^= state >> 29;
    format!("{state:016x}")
}

fn describe_class(class: Option<MapClass>) -> String {
    class.map_or_else(
        || "an unclassified extension".to_owned(),
        |class| format!("a {}", class.description()),
    )
}

/// Load the `.til` and its atlas for one map.
///
/// The archive form resolves the member name through [`resolve_tileset`] and refuses anything that
/// does not resolve to exactly one, because "combat map, no binding" is a finding and picking one
/// would be a guess. The loose form takes the caller's word for the file but still runs
/// [`tileset_mismatch`], so a shipped tileset the engine would not use here is refused.
fn load_tile_set(
    map_path: &Path,
    source: &TileSetSource,
    notes: &mut Vec<String>,
) -> Result<LoadedTileSet, String> {
    let resolution = resolve_tileset(map_path);
    match source {
        TileSetSource::Archive(archive_path) => {
            let Some(resolution) = resolution else {
                return Err(format!(
                    "could not resolve a tileset for {}",
                    map_path.display()
                ));
            };
            let member = match &resolution {
                TileSetResolution::World(members) | TileSetResolution::Combat(members) => members[0],
                TileSetResolution::CombatAmbiguous(members) => {
                    return Err(format!(
                        "different encounters read {} through {}, so there is no single answer. \
                         Pick the one matching the encounter you are editing and run --serve with \
                         that .til and its atlas instead of --pic",
                        map_path.display(),
                        members.join(" or "),
                    ));
                }
                TileSetResolution::CombatUnresolved => {
                    return Err(format!(
                        "no gamescript encounter binds {} to a tileset, so this project does not \
                         know which one it uses and will not guess. 168 of the 337 installed .smp \
                         files are in this position, and scoring cannot settle it: the 26 shipped \
                         tilesets collapse to 16 distinct rule sets. Run --serve with an explicit \
                         .til and its atlas if you know which one you mean",
                        map_path.display(),
                    ));
                }
            };
            let archive = Archive::open(archive_path)
                .map_err(|error| format!("could not open {}: {error}", archive_path.display()))?;
            let definition_bytes = archive
                .read(&format!("til\\{member}"))
                .map_err(|error| format!("could not read til\\{member}: {error}"))?;
            let definition = TileSetDefinition::parse(&definition_bytes)
                .map_err(|error| format!("could not parse til\\{member}: {error}"))?;
            let atlas_member = format!("til\\{}", definition.atlas_member);
            let atlas_bytes = archive
                .read(&atlas_member)
                .map_err(|error| format!("could not read {atlas_member}: {error}"))?;
            let atlas = decode_atlas(&atlas_bytes, &atlas_member, &definition)?;
            notes.push(format!(
                "tileset {member} and atlas {} read from {}",
                definition.atlas_member,
                archive_path.display()
            ));
            Ok(LoadedTileSet {
                member: member.to_owned(),
                provenance: format!("gamescript binding: {}", resolution.describe()),
                definition,
                atlas,
            })
        }
        TileSetSource::Loose { definition, atlas } => {
            if let Some(mismatch) = tileset_mismatch(map_path, definition) {
                return Err(format!(
                    "refusing to open {}: {mismatch}",
                    map_path.display()
                ));
            }
            let definition_bytes = fs::read(definition).map_err(|error| {
                format!("could not read tile definition {}: {error}", definition.display())
            })?;
            let parsed = TileSetDefinition::parse(&definition_bytes).map_err(|error| {
                format!("could not parse tile definition {}: {error}", definition.display())
            })?;
            let atlas_bytes = fs::read(atlas)
                .map_err(|error| format!("could not read tile atlas {}: {error}", atlas.display()))?;
            let decoded = decode_atlas(&atlas_bytes, &atlas.display().to_string(), &parsed)?;
            let member = definition
                .file_name()
                .map_or_else(|| definition.display().to_string(), |name| {
                    name.to_string_lossy().into_owned()
                });
            let provenance = match &resolution {
                Some(resolution) if resolution.accepts(&member) => {
                    format!("supplied, and the gamescript binding agrees: {}", resolution.describe())
                }
                Some(resolution) => {
                    notes.push(format!(
                        "the gamescript binding for {} is {}; {member} is not one of the 26 \
                         shipped tilesets, so it is presumed modded and accepted as supplied",
                        map_path.display(),
                        resolution.describe(),
                    ));
                    format!("supplied; gamescript binding: {}", resolution.describe())
                }
                None => "supplied; this map has no gamescript binding to check it against"
                    .to_owned(),
            };
            Ok(LoadedTileSet {
                member,
                provenance,
                definition: parsed,
                atlas: decoded,
            })
        }
    }
}

/// Decode an atlas and check it against the geometry the `.til` declares.
///
/// The same check `--view-map` makes. An atlas of the wrong size would not fail loudly -- it would
/// draw the map out of a tileset it does not belong to.
fn decode_atlas(
    bytes: &[u8],
    name: &str,
    definition: &TileSetDefinition,
) -> Result<PbmImage, String> {
    let atlas =
        PbmImage::decode(bytes).map_err(|error| format!("could not decode {name}: {error}"))?;
    let expected_width = definition
        .columns
        .checked_mul(definition.tile_width)
        .ok_or_else(|| "tile atlas width overflow".to_owned())?;
    let expected_height = definition
        .rows
        .checked_mul(definition.tile_height)
        .ok_or_else(|| "tile atlas height overflow".to_owned())?;
    if u32::from(atlas.width) != expected_width || u32::from(atlas.height) != expected_height {
        return Err(format!(
            "tile atlas {name} is {}x{}, but its tileset declares {expected_width}x{expected_height}",
            atlas.width, atlas.height,
        ));
    }
    Ok(atlas)
}

/// The metadata, grid and palette one open map answers with.
///
/// **The terrain palette is built from the tileset's own tiles**, not from the eleven-name world
/// table: terrain ids are tileset-local and combat tilesets reach 42. A terrain the file declares
/// under `TERRAINTYPE=` but draws with no tile is listed and marked unpaintable, because that is
/// exactly what `plan_terrain_paint` refuses and the palette must not offer what the writer will
/// reject.
fn open_json(
    path: &Path,
    class: MapClass,
    map: &MapAsset,
    tile_set: &LoadedTileSet,
    token: &str,
    notes: &[String],
) -> String {
    let mut tiles_by_terrain: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for tile in tile_set.definition.tiles.values() {
        tiles_by_terrain
            .entry(tile.terrain_type)
            .or_default()
            .push(tile.index);
    }
    let mut terrain_ids: Vec<u32> = tiles_by_terrain.keys().copied().collect();
    for declared in tile_set.definition.terrain_types.keys() {
        if !tiles_by_terrain.contains_key(declared) {
            terrain_ids.push(*declared);
        }
    }
    terrain_ids.sort_unstable();
    terrain_ids.dedup();

    let mut terrains = String::from("[");
    for (position, id) in terrain_ids.iter().enumerate() {
        if position > 0 {
            terrains.push(',');
        }
        let slots = tiles_by_terrain.get(id);
        let description = tile_set
            .definition
            .terrain_types
            .get(id)
            .map_or_else(|| format!("terrain {id}"), |terrain| terrain.description.clone());
        let _ = write!(
            terrains,
            "{{\"id\":{id},\"description\":{},\"tiles\":{},\"swatch\":{},\"paintable\":{}}}",
            json_string(&description),
            slots.map_or(0, Vec::len),
            slots.and_then(|slots| slots.iter().min().copied()).unwrap_or(0),
            slots.is_some(),
        );
    }
    terrains.push(']');

    format!(
        "{{\"ok\":true,\"path\":{},\"token\":{},\"width\":{},\"height\":{},\"class\":{},\
         \"tileset\":{{\"member\":{},\"provenance\":{},\"atlas\":{},\"columns\":{},\"rows\":{},\
         \"tileWidth\":{},\"tileHeight\":{}}},\"terrains\":{terrains},\"tiles\":{},\"notes\":{}}}",
        json_string(&path.display().to_string()),
        json_string(token),
        map.width,
        map.height,
        json_string(class.description()),
        json_string(&tile_set.member),
        json_string(&tile_set.provenance),
        json_string(&tile_set.definition.atlas_member),
        tile_set.definition.columns,
        tile_set.definition.rows,
        tile_set.definition.tile_width,
        tile_set.definition.tile_height,
        tiles_json(map),
        json_strings(notes),
    )
}

/// The cell grid, in the map file's own packed `y * width + x` order.
fn tiles_json(map: &MapAsset) -> String {
    let mut tiles = String::with_capacity(map.cells.len() * 4 + 2);
    tiles.push('[');
    for (position, cell) in map.cells.iter().enumerate() {
        if position > 0 {
            tiles.push(',');
        }
        let _ = write!(tiles, "{}", cell.tile_index());
    }
    tiles.push(']');
    tiles
}

fn json_strings(values: &[String]) -> String {
    let mut out = String::from("[");
    for (position, value) in values.iter().enumerate() {
        if position > 0 {
            out.push(',');
        }
        out.push_str(&json_string(value));
    }
    out.push(']');
    out
}

/// Escape a string for JSON.
///
/// Refusal text carries map paths and tileset names straight from the filesystem, so this escapes
/// the control range as well as the two structural characters -- a path with a quote or a newline
/// in it would otherwise produce a body the client cannot parse and a refusal the user never sees.
fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if (control as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", control as u32);
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// Parse `a=1&b=two` into fields, percent-decoding both halves.
///
/// Used for query strings and for `application/x-www-form-urlencoded` bodies, which are the same
/// grammar. Paths are the reason this cannot be a naive split: the installed corpus lives under
/// `Program Files (x86)`, with spaces and parentheses in it.
pub fn form_fields(source: &str) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    for pair in source.split('&').filter(|pair| !pair.is_empty()) {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        fields.insert(percent_decode(name), percent_decode(value));
    }
    fields
}

/// Percent-decode over **bytes**.
///
/// **Never slice the `&str`.** `&value[index + 1..index + 3]` panics when those offsets fall inside
/// a multi-byte character, and `%` followed by any non-ASCII byte does exactly that: a POST body of
/// `x0=%` plus a euro sign took the whole process down, and with it every unsaved paint in the held
/// session. A malformed escape is not an error here -- there is no way to tell a stray `%` in a
/// filename from a broken one -- so it is emitted literally and the cursor advances one byte.
fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            b'%' => {
                match bytes
                    .get(index + 1)
                    .copied()
                    .and_then(hex_digit)
                    .zip(bytes.get(index + 2).copied().and_then(hex_digit))
                {
                    Some((high, low)) => {
                        out.push(high * 16 + low);
                        index += 3;
                    }
                    None => {
                        out.push(b'%');
                        index += 1;
                    }
                }
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// One hexadecimal digit's value, or `None` for any other byte.
const fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Bind the editor's listener, on loopback and nowhere else.
///
/// **`127.0.0.1`, never `0.0.0.0`.** This process reads and writes arbitrary local files on
/// request, so it must not be reachable off-host; the address is built here from literal octets
/// rather than parsed from anything the caller supplies, so no argument can widen it. Split out
/// from [`serve`] so a test can bind port 0 and drive the real socket.
pub fn listen(port: u16) -> Result<(tiny_http::Server, SocketAddr), String> {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let server = tiny_http::Server::http(address)
        .map_err(|error| format!("could not listen on {address}: {error}"))?;
    let bound = server
        .server_addr()
        .to_ip()
        .ok_or_else(|| "the server did not bind an IP address".to_owned())?;
    Ok((server, bound))
}

/// One request header's value, matched case-insensitively as HTTP requires.
fn header_value(request: &tiny_http::Request, name: &'static str) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|header| header.field.equiv(name))
        .map(|header| header.value.as_str().to_owned())
}

/// The most a request body may be, in bytes.
///
/// A save path and a paint rectangle are tens of bytes. An unbounded `read_to_string` on a socket
/// anyone's web page can open is a free way to make this process allocate until it dies.
const MAX_BODY_BYTES: u64 = 64 * 1024;

/// Answer requests on `server` until it stops yielding them.
///
/// **A handler panic must not take the session with it.** This server is single-threaded by design
/// and holds the user's unsaved paints in memory, so before `catch_unwind` any panic anywhere in a
/// handler ended the process and threw away everything not yet written -- a malformed percent
/// escape in a request body did exactly that. The panic is still a bug and still prints; what
/// changes is that it costs one request instead of the session. The caught state is not fully
/// unwind-safe in the type-system sense, and that is an accepted trade: the editor's mutations are
/// whole-value assignments, so the worst outcome is a map left as it was before the failed request.
pub fn run(server: &tiny_http::Server, source: TileSetSource, port: u16) {
    let mut editor = Editor::new(source, port);
    run_with(server, |request| editor.handle(request));
}

/// The socket loop, over any handler.
///
/// Split from [`run`] so a test can supply a handler that panics. With the one known panic fixed
/// there is nothing left in the editor that panics on demand, and a test that only sends the
/// malformed bytes proves the decoder and says nothing at all about whether the loop survives the
/// *next* bug -- which is the property `catch_unwind` is here for.
pub fn run_with(
    server: &tiny_http::Server,
    mut handler: impl FnMut(&HttpRequest<'_>) -> HttpResponse,
) {
    for mut request in server.incoming_requests() {
        let method = request.method().as_str().to_owned();
        let target = request.url().to_owned();
        let host = header_value(&request, "Host");
        let origin = header_value(&request, "Origin");
        let mut body = String::new();
        if let Err(error) = request
            .as_reader()
            .take(MAX_BODY_BYTES)
            .read_to_string(&mut body)
        {
            eprintln!("could not read the request body: {error}");
            continue;
        }
        let parsed = HttpRequest {
            method: &method,
            target: &target,
            body: &body,
            host: host.as_deref(),
            origin: origin.as_deref(),
        };
        let response = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            handler(&parsed)
        }))
        .unwrap_or_else(|_| {
            eprintln!(
                "a handler panicked on {method} {target}; the open map is untouched and the \
                 editor is still running"
            );
            HttpResponse {
                status: 500,
                content_type: "application/json; charset=utf-8",
                body: br#"{"ok":false,"refusal":"the editor hit a bug handling that request. It is still running and the open map is untouched; please report what you did.","notes":[]}"#
                    .to_vec(),
            }
        });
        let header = tiny_http::Header::from_bytes(&b"Content-Type"[..], response.content_type)
            .expect("the content types are static and valid header values");
        let reply = tiny_http::Response::from_data(response.body)
            .with_status_code(response.status)
            .with_header(header);
        if let Err(error) = request.respond(reply) {
            eprintln!("could not send the response: {error}");
        }
    }
}

/// Serve the editor on loopback until the process is killed.
pub fn serve(source: TileSetSource, port: u16) -> Result<(), String> {
    let (server, bound) = listen(port)?;
    println!("map editor listening on http://{bound}/");
    println!("loopback only; it reads and writes local files, so do not expose it");
    run(&server, source, bound.port());
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::{Read as _, Write as _};
    use std::net::{SocketAddr, TcpStream};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::thread;

    use super::*;

    /// A synthetic tileset, written to a scratch file so the real `.til` parser runs.
    ///
    /// **No game asset is committed.** It is built to be *unlike* the shipped world tileset in the
    /// three ways that have caused bugs here:
    ///
    /// - a terrain id of **42**, past the eleven-name world table, because ids are tileset-local
    ///   and combat tilesets really do reach 42;
    /// - terrain **3**, declared under `TERRAINTYPE=` and drawn by no tile, which is exactly what
    ///   `plan_terrain_paint` refuses -- so a palette that offers it is offering something the
    ///   writer will reject;
    /// - terrain 2 with **three** interchangeable interior tiles, so a painted interior is a draw
    ///   the engine would have made at random and this tool has to say so.
    ///
    /// The grass/stone ring is the one `--map-paint-terrain`'s own CLI test uses, because it is
    /// asymmetric in all eight directions: N, S, E, W and each diagonal take a different tile, so a
    /// mirrored or transposed direction convention fails here instead of passing by symmetry.
    const FIXTURE_TILESET: &[u8] = br#"
LBM=fixture.lbm
TILES= 12, 3
TILESIZE= 8, 8
TERRAINTYPE= 1, 40, "grass",  0, 100, 200, 11, 12, 5, 13, 14
TERRAINTYPE= 2, 41, "stone",  2, 300, 400, 21, 22, 9, 23, 24
TERRAINTYPE= 3, 42, "path",   0, 0, 9999, 0, 0, 1, 0, 0
TERRAINTYPE= 42, 44, "deep cave", 2, 0, 9999, 0, 0, 3, 0, 0
;         self, n,    ne,  e,    se,  s,    sw,  w,    nw,   index
TILE=  0,    1, 1,    1,   1,    1,   1,    1,   1,    1,    0
TILE=  3,    1, 2,    *,   1,    *,   1,    *,   1,    *,    3
TILE=  4,    1, 1,    *,   1,    *,   2,    *,   1,    *,    4
TILE=  5,    1, 1,    *,   1,    *,   1,    *,   2,    *,    5
TILE=  6,    1, 1,    *,   2,    *,   1,    *,   1,    *,    6
TILE=  7,    1, 1,    1,   1,    1,   1,    1,   1,    2,    7
TILE=  8,    1, 1,    2,   1,    1,   1,    1,   1,    1,    8
TILE=  9,    1, 1,    1,   1,    1,   1,    2,   1,    1,    9
TILE= 10,    1, 1,    1,   1,    2,   1,    1,   1,    1,    10
TILE= 11,    2, ~1,   ~1,  ~1,   ~1,  ~1,   ~1,  ~1,   ~1,   11
TILE= 12,    2, 1,    1,   1,    1,   1,    1,   1,    1,    12
TILE= 13,    2, 1,    1,   2,    1,   1,    1,   1,    1,    13
TILE= 14,    2, 1,    1,   1,    1,   1,    1,   2,    1,    14
TILE= 19,    2, 1,    *,   2,    *,   2,    *,   2,    *,    19
TILE= 20,    2, 2,    *,   2,    *,   1,    *,   2,    *,    20
TILE= 21,    2, 2,    *,   2,    *,   2,    *,   1,    *,    21
TILE= 22,    2, 2,    *,   1,    *,   2,    *,   2,    *,    22
TILE= 23,    2, 2,    2,   2,    1,   2,    2,   2,    2,    23
TILE= 24,    2, 2,    2,   2,    2,   2,    1,   2,    2,    24
TILE= 25,    2, 2,    1,   2,    2,   2,    2,   2,    2,    25
TILE= 26,    2, 2,    2,   2,    2,   2,    2,   2,    1,    26
TILE= 27,    2, ~1,   ~1,  ~1,   ~1,  ~1,   ~1,  ~1,   ~1,   27
TILE= 28,    2, ~1,   ~1,  ~1,   ~1,  ~1,   ~1,  ~1,   ~1,   28
TILE= 29,    2, 1,    *,   2,    *,   2,    *,   1,    *,    29
TILE= 30,    2, 1,    *,   1,    *,   2,    *,   2,    *,    30
TILE= 31,    2, 2,    *,   2,    *,   1,    *,   1,    *,    31
TILE= 32,    2, 2,    *,   1,    *,   1,    *,   2,    *,    32
TILE= 33,   42, *,    *,   *,    *,   *,    *,   *,    *,    33
TILE= 34,   42, *,    *,   *,    *,   *,    *,   *,    *,    34
TILE= 35,   42, *,    *,   *,    *,   *,    *,   *,    *,    35
"#;

    const FIXTURE_COLUMNS: u32 = 12;
    const FIXTURE_ROWS: u32 = 3;
    const FIXTURE_TILE: u32 = 8;
    /// Deliberately **not square**: every shipped world map is, and a square fixture cannot fail on
    /// a transposed cell index. 11x5 makes `y * width + x` and `x * height + y` disagree everywhere
    /// off the diagonal.
    const FIXTURE_WIDTH: u32 = 11;
    const FIXTURE_HEIGHT: u32 = 5;

    /// An uncompressed IFF `FORM PBM` whose palette index at every pixel is its own atlas slot.
    ///
    /// Built through the real decoder rather than by constructing a `PbmImage` directly, so the
    /// atlas endpoint is exercised end to end, and slot-coloured so a served atlas that is scaled,
    /// transposed or offset gives a different pixel than the one asserted.
    fn fixture_atlas(columns: u32, rows: u32) -> Vec<u8> {
        let width = (columns * FIXTURE_TILE) as u16;
        let height = (rows * FIXTURE_TILE) as u16;
        let row_bytes = (usize::from(width) + 1) & !1;
        let mut body = vec![0_u8; row_bytes * usize::from(height)];
        for y in 0..usize::from(height) {
            for x in 0..usize::from(width) {
                let slot = (y / FIXTURE_TILE as usize) * columns as usize + x / FIXTURE_TILE as usize;
                body[y * row_bytes + x] = u8::try_from(slot % 256).unwrap();
            }
        }
        let mut palette = Vec::with_capacity(768);
        for index in 0..256_u32 {
            palette.push(u8::try_from(index).unwrap());
            palette.push(u8::try_from(255 - index).unwrap());
            palette.push(u8::try_from((index * 7) % 256).unwrap());
        }

        let mut bmhd = Vec::new();
        bmhd.extend_from_slice(&width.to_be_bytes());
        bmhd.extend_from_slice(&height.to_be_bytes());
        bmhd.extend_from_slice(&[0; 4]); // x, y origin
        bmhd.push(8); // planes
        bmhd.push(0); // pad
        bmhd.push(0); // masking
        bmhd.push(0); // compression: none
        bmhd.extend_from_slice(&[0, 0]); // pad
        bmhd.extend_from_slice(&[0, 0]); // transparent colour
        bmhd.extend_from_slice(&[1, 1]); // aspect
        bmhd.extend_from_slice(&width.to_be_bytes());
        bmhd.extend_from_slice(&height.to_be_bytes());

        let mut form = Vec::new();
        form.extend_from_slice(b"PBM ");
        for (id, chunk) in [
            (b"BMHD", bmhd),
            (b"CMAP", palette),
            (b"BODY", body),
        ] {
            form.extend_from_slice(id);
            form.extend_from_slice(&u32::try_from(chunk.len()).unwrap().to_be_bytes());
            form.extend_from_slice(&chunk);
            if chunk.len() % 2 == 1 {
                form.push(0);
            }
        }
        let mut bytes = Vec::from(*b"FORM");
        bytes.extend_from_slice(&u32::try_from(form.len()).unwrap().to_be_bytes());
        bytes.extend_from_slice(&form);
        bytes
    }

    /// A map of uniform grass, in the 47-byte-tail shape the CLI's own paint fixture uses.
    fn grass_map(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0x6f_u32.to_le_bytes());
        bytes.extend_from_slice(&width.to_le_bytes());
        bytes.extend_from_slice(&height.to_le_bytes());
        bytes.extend_from_slice(&8_u32.to_le_bytes());
        for index in 0..width * height {
            bytes.extend_from_slice(&0_u32.to_le_bytes());
            bytes.extend_from_slice(&(index as f32).to_le_bytes());
        }
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes
    }

    static SCRATCH_COUNTER: AtomicU32 = AtomicU32::new(0);

    fn scratch_dir(name: &str) -> PathBuf {
        let unique = SCRATCH_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "lom-server-{name}-{}-{unique}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A scratch directory holding a map, a tileset and an atlas, plus an editor already open on it.
    struct Fixture {
        dir: PathBuf,
        map: PathBuf,
        editor: Editor,
        /// The handle the last successful open handed out.
        token: String,
    }

    /// The port every direct-handler fixture pretends to be bound to.
    const FIXTURE_PORT: u16 = 8731;

    /// A request from the editor's own page: the shape the browser guard must let through.
    fn own_page<'a>(method: &'a str, target: &'a str, body: &'a str) -> HttpRequest<'a> {
        HttpRequest {
            method,
            target,
            body,
            host: Some("127.0.0.1:8731"),
            origin: Some("http://127.0.0.1:8731"),
        }
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            Self::with_atlas(name, FIXTURE_COLUMNS, FIXTURE_ROWS)
        }

        /// A fixture on a map of a chosen size, for tests that need room to paint without the
        /// rings of two paints meeting.
        fn with_map(name: &str, width: u32, height: u32) -> Self {
            let fixture = Self::new(name);
            fs::write(&fixture.map, grass_map(width, height)).unwrap();
            fixture
        }

        fn with_atlas(name: &str, columns: u32, rows: u32) -> Self {
            let dir = scratch_dir(name);
            let map = dir.join("in.scn");
            fs::write(&map, grass_map(FIXTURE_WIDTH, FIXTURE_HEIGHT)).unwrap();
            fs::write(dir.join("fixture.til"), FIXTURE_TILESET).unwrap();
            fs::write(dir.join("fixture.lbm"), fixture_atlas(columns, rows)).unwrap();
            let editor = Editor::new(
                TileSetSource::Loose {
                    definition: dir.join("fixture.til"),
                    atlas: dir.join("fixture.lbm"),
                },
                FIXTURE_PORT,
            );
            Self {
                dir,
                map,
                editor,
                token: String::new(),
            }
        }

        fn open(&mut self) -> String {
            self.open_path(&self.map.display().to_string().clone())
        }

        fn open_path(&mut self, path: &str) -> String {
            let body = format!("path={}", encode(path));
            let response = self.editor.handle(&own_page("POST", "/api/open", &body));
            let json = String::from_utf8(response.body).unwrap();
            if let Some(start) = json.find("\"token\":\"") {
                let start = start + "\"token\":\"".len();
                self.token = json[start..start + json[start..].find('"').unwrap()].to_owned();
            }
            json
        }

        /// A POST carrying this tab's handle, the way the page does.
        fn post(&mut self, path: &str, body: &str) -> String {
            let body = if body.is_empty() {
                format!("token={}", self.token)
            } else {
                format!("{body}&token={}", self.token)
            };
            let response = self.editor.handle(&own_page("POST", path, &body));
            String::from_utf8(response.body).unwrap()
        }

        fn paint(&mut self, rect: (u32, u32, u32, u32), terrain: u32, seed: Option<u64>) -> String {
            let (x0, y0, x1, y1) = rect;
            let mut body = format!("x0={x0}&y0={y0}&x1={x1}&y1={y1}&terrain={terrain}");
            if let Some(seed) = seed {
                let _ = write!(body, "&seed={seed}");
            }
            self.post("/api/paint", &body)
        }

        fn save(&mut self, target: &Path) -> String {
            self.post(
                "/api/save",
                &format!("path={}", encode(&target.display().to_string())),
            )
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    /// Percent-encode everything that is not unreserved, the way `encodeURIComponent` does.
    fn encode(value: &str) -> String {
        let mut out = String::new();
        for byte in value.bytes() {
            if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
                out.push(char::from(byte));
            } else {
                let _ = write!(out, "%{byte:02X}");
            }
        }
        out
    }

    /// The value of a top-level `"name": <number>` field, without a JSON parser.
    fn number_field(json: &str, name: &str) -> i64 {
        let needle = format!("\"{name}\":");
        let start = json.find(&needle).expect(name) + needle.len();
        let rest = &json[start..];
        let end = rest
            .find(|character: char| !character.is_ascii_digit() && character != '-')
            .unwrap_or(rest.len());
        rest[..end].parse().unwrap()
    }

    /// The `"tiles": [..]` array.
    fn tiles_field(json: &str) -> Vec<u32> {
        let start = json.find("\"tiles\":[").expect("tiles") + "\"tiles\":[".len();
        let end = start + json[start..].find(']').unwrap();
        json[start..end]
            .split(',')
            .filter(|value| !value.is_empty())
            .map(|value| value.parse().unwrap())
            .collect()
    }

    /// The `{"i":N,"tile":M}` pairs a paint answered with.
    fn painted_cells(json: &str) -> BTreeMap<u32, u32> {
        let mut cells = BTreeMap::new();
        let mut rest = json;
        while let Some(start) = rest.find("{\"i\":") {
            rest = &rest[start + 5..];
            let (index, tail) = rest.split_once(",\"tile\":").unwrap();
            let (tile, tail) = tail.split_once('}').unwrap();
            cells.insert(index.parse().unwrap(), tile.parse().unwrap());
            rest = tail;
        }
        cells
    }

    /// The `"refusal"` string, whatever else the object carries.
    ///
    /// Reads to the closing quote rather than to a following key, because an open's refusal also
    /// reports what the editor is still holding and a field-order assumption would break silently.
    fn refusal(json: &str) -> String {
        assert!(json.contains("\"ok\":false"), "expected a refusal: {json}");
        let start = json.find("\"refusal\":\"").expect("refusal") + "\"refusal\":\"".len();
        let rest = &json[start..];
        let mut end = 0;
        let bytes = rest.as_bytes();
        while end < bytes.len() && bytes[end] != b'"' {
            end += if bytes[end] == b'\\' { 2 } else { 1 };
        }
        rest[..end].to_owned()
    }

    #[test]
    fn a_form_body_round_trips_a_path_with_spaces_and_parentheses() {
        // The installed corpus lives under `Program Files (x86)`. A decoder that stops at the
        // reserved set, or that splits on `=` more than once, loses the map before anything else
        // can go wrong.
        let path = "/Users/a b/Program Files (x86)/Lords of Magic/map/URAK.scn";
        let fields = form_fields(&format!("path={}&terrain=6", encode(path)));
        assert_eq!(fields.get("path").map(String::as_str), Some(path));
        assert_eq!(fields.get("terrain").map(String::as_str), Some("6"));
        // `+` is a space in this grammar, and an escaped `+` is a literal one.
        let plus = form_fields("path=a+b%2Bc");
        assert_eq!(plus.get("path").map(String::as_str), Some("a b+c"));
    }

    #[test]
    fn the_page_and_its_assets_come_out_of_the_binary_and_nothing_else_does() {
        let mut editor = Editor::new(
            TileSetSource::Archive(PathBuf::from("/nonexistent.mpq")),
            FIXTURE_PORT,
        );
        let page = editor.handle(&own_page("GET", "/", ""));
        assert_eq!(page.status, 200);
        assert!(String::from_utf8(page.body).unwrap().contains("<canvas id=\"map\">"));
        assert_eq!(editor.handle(&own_page("GET", "/app.js", "")).status, 200);
        assert_eq!(editor.handle(&own_page("GET", "/style.css", "")).status, 200);
        // Nothing else is reachable: this process reads and writes local files, so an
        // unrecognised path must not become a file read.
        let missing = editor.handle(&own_page("GET", "/../../etc/passwd", ""));
        assert_eq!(missing.status, 404);
        assert_eq!(editor.handle(&own_page("POST", "/", "")).status, 404);
        // The old state-mutating GET is gone, not merely unused by our page.
        assert_eq!(
            editor.handle(&own_page("GET", "/api/open?path=/etc/hosts", "")).status,
            404,
            "opening a map by GET is reachable from a bare <img> tag"
        );
    }

    #[test]
    fn opening_a_non_square_map_reports_the_grid_in_the_file_s_own_packed_order() {
        let mut fixture = Fixture::new("open");
        // Distinct tiles down column 0 and along row 0, on an 11x5 map, so `y * width + x` and
        // `x * height + y` disagree: a transposed index puts tile 5 at position 55, which is off
        // the end of a 55-cell grid, and tile 3 at position 15 rather than 3.
        let mut bytes = grass_map(FIXTURE_WIDTH, FIXTURE_HEIGHT);
        let mut set = |x: u32, y: u32, tile: u32| {
            let offset = 16 + ((y * FIXTURE_WIDTH + x) as usize) * 8;
            bytes[offset..offset + 4].copy_from_slice(&tile.to_le_bytes());
        };
        set(3, 0, 3);
        set(0, 4, 4);
        set(10, 4, 5);
        fs::write(&fixture.map, &bytes).unwrap();

        let json = fixture.open();
        assert!(json.contains("\"ok\":true"), "{json}");
        assert_eq!(number_field(&json, "width"), 11);
        assert_eq!(number_field(&json, "height"), 5);
        let tiles = tiles_field(&json);
        assert_eq!(tiles.len(), 55);
        assert_eq!(tiles[3], 3, "(3, 0)");
        assert_eq!(tiles[4 * 11], 4, "(0, 4)");
        assert_eq!(tiles[4 * 11 + 10], 5, "(10, 4)");
        assert!(json.contains("\"class\":\"world map\""), "{json}");
        assert!(json.contains("\"columns\":12"), "{json}");
        assert!(json.contains("\"tileWidth\":8"), "{json}");
    }

    #[test]
    fn the_terrain_palette_is_the_tileset_s_own_and_not_the_eleven_name_world_table() {
        let mut fixture = Fixture::new("palette");
        let json = fixture.open();
        // Terrain 42 is the whole point: a hardcoded 0..10 world palette cannot produce it, and
        // combat tilesets really do reach 42.
        assert!(
            json.contains("{\"id\":42,\"description\":\"deep cave\",\"tiles\":3,\"swatch\":33,\"paintable\":true}"),
            "{json}"
        );
        // Declared under TERRAINTYPE= and drawn by no tile. Offering it would offer a paint the
        // writer refuses, so it is listed and marked unpaintable rather than dropped or offered.
        assert!(
            json.contains("{\"id\":3,\"description\":\"path\",\"tiles\":0,\"swatch\":0,\"paintable\":false}"),
            "{json}"
        );
        assert!(json.contains("\"description\":\"grass\""), "{json}");
        // The world table's names must not appear: this tileset declares none of them.
        for world_name in ["water", "desert", "mountain", "happy plains", "impassable"] {
            assert!(
                !json.contains(&format!("\"description\":\"{world_name}\"")),
                "the world terrain table leaked into the palette: {json}"
            );
        }
    }

    #[test]
    fn the_atlas_is_served_as_a_png_at_the_geometry_the_tileset_declares() {
        let mut fixture = Fixture::new("atlas");
        fixture.open();
        let target = format!("/api/atlas.png?token={}", fixture.token);
        let response = fixture.editor.handle(&own_page("GET", &target, ""));
        assert_eq!(response.content_type, "image/png");
        let decoder = png::Decoder::new(std::io::Cursor::new(&response.body));
        let mut reader = decoder.read_info().unwrap();
        let info = reader.info().clone();
        assert_eq!((info.width, info.height), (96, 24));
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        let frame = reader.next_frame(&mut pixels).unwrap();
        let pixels = &pixels[..frame.buffer_size()];
        // The client turns a tile index into `(index % columns, index / columns)` atlas cells. Slot
        // 13 is column 1, row 1, so its centre is (12, 12); the fixture paints every pixel of a
        // slot with that slot's palette index. A served atlas that is scaled, transposed, or drawn
        // at the wrong row stride reads a different colour here.
        let offset = ((12 * 96) + 12) * 4;
        assert_eq!(&pixels[offset..offset + 3], &[13_u8, 242, 91]);
    }

    #[test]
    fn an_atlas_that_does_not_match_the_tileset_s_geometry_is_refused_rather_than_drawn() {
        // A wrong-sized atlas does not fail loudly on its own -- it silently draws the map out of
        // artwork it does not belong to.
        let mut fixture = Fixture::with_atlas("atlas-mismatch", FIXTURE_COLUMNS, 2);
        let json = fixture.open();
        let refusal = refusal(&json);
        assert!(refusal.contains("96x16"), "{refusal}");
        assert!(refusal.contains("96x24"), "{refusal}");
    }

    #[test]
    fn a_paint_answers_with_the_cells_it_changed_at_their_packed_indexes() {
        let mut fixture = Fixture::new("paint");
        fixture.open();
        // One stone cell in a grass field at (5, 2) of an 11x5 map. N, S, E, W and all four
        // diagonals take different tiles, so a mirrored convention or a transposed index fails.
        let json = fixture.paint((5, 2, 5, 2), 2, None);
        assert!(json.contains("\"ok\":true"), "{json}");
        let cells = painted_cells(&json);
        let at = |x: u32, y: u32| y * FIXTURE_WIDTH + x;
        assert_eq!(cells.get(&at(5, 2)), Some(&12), "region");
        assert_eq!(cells.get(&at(5, 1)), Some(&4), "N");
        assert_eq!(cells.get(&at(5, 3)), Some(&3), "S");
        assert_eq!(cells.get(&at(4, 2)), Some(&6), "W");
        assert_eq!(cells.get(&at(6, 2)), Some(&5), "E");
        assert_eq!(cells.get(&at(4, 1)), Some(&10), "NW");
        assert_eq!(cells.get(&at(6, 1)), Some(&9), "NE");
        assert_eq!(cells.get(&at(4, 3)), Some(&8), "SW");
        assert_eq!(cells.get(&at(6, 3)), Some(&7), "SE");
        assert_eq!(cells.len(), 9, "{json}");
        // Nothing was drawn at random here: every cell was determined by the tileset alone, so
        // the unreproducibility note must be absent. It is the note firing that would be the lie.
        assert!(!json.contains("legal choice"), "{json}");
    }

    #[test]
    fn an_interior_the_engine_would_have_drawn_at_random_says_so_in_the_notes() {
        let mut fixture = Fixture::new("drawn");
        fixture.open();
        // A 3x3 stone region: the centre's neighbourhood is all stone, which three interchangeable
        // interior tiles accept, and the cell held none of them. That is the case the engine
        // decides by a draw this project cannot reproduce.
        let json = fixture.paint((4, 1, 6, 3), 2, None);
        assert!(json.contains("\"ok\":true"), "{json}");
        assert!(json.contains("drawn:1\""), "{json}");
        // The ring of this region reaches rows 0 and 4 of a 5-row map, so the off-map neighbour
        // assumption -- which no saved artifact tests -- was leaned on and has to be said out loud.
        assert!(json.contains("neighbour off the map"), "{json}");
        assert!(
            json.contains("legal choice, not the engine's"),
            "an unreproducible cell was painted and not reported: {json}"
        );
        let cells = painted_cells(&json);
        let centre = cells[&(2 * FIXTURE_WIDTH + 5)];
        assert!([11, 27, 28].contains(&centre), "centre tile {centre}");
    }

    #[test]
    fn the_seed_reaches_the_draw_and_the_same_seed_gives_the_same_map() {
        let centre = 2 * FIXTURE_WIDTH + 5;
        let tile_for = |seed: Option<u64>| {
            let mut fixture = Fixture::new("seed");
            fixture.open();
            painted_cells(&fixture.paint((4, 1, 6, 3), 2, seed))[&centre]
        };
        let default = tile_for(None);
        assert_eq!(default, 11, "the default is the lowest matching slot");
        let seeds: Vec<u32> = (0..64).map(|seed| tile_for(Some(seed))).collect();
        assert!(
            seeds.iter().any(|tile| *tile != default),
            "no seed changed the draw, so the seed never reached the selector: {seeds:?}"
        );
        assert!(
            seeds.iter().all(|tile| [11, 27, 28].contains(tile)),
            "a seed chose a tile outside the candidate set: {seeds:?}"
        );
        assert_eq!(tile_for(Some(7)), tile_for(Some(7)), "one seed, one map");
    }

    #[test]
    fn a_refused_paint_is_readable_text_and_leaves_the_map_exactly_as_it_was() {
        let mut fixture = Fixture::new("refused");
        fixture.open();
        let before = fixture.editor.session().unwrap().map.clone();

        let outside = refusal(&fixture.paint((9, 3, 12, 4), 2, None));
        assert!(outside.contains("(9, 3)..(12, 4)"), "{outside}");
        assert!(outside.contains("11x5"), "{outside}");

        // Terrain 3 is declared and drawn by no tile. The message has to name the terrain, not
        // just say no.
        let undrawn = refusal(&fixture.paint((1, 1, 2, 2), 3, None));
        assert!(undrawn.contains("no tiles for terrain type 3"), "{undrawn}");

        let backwards = refusal(&fixture.paint((4, 4, 2, 1), 2, None));
        assert!(backwards.contains("is not a rectangle"), "{backwards}");

        assert_eq!(
            fixture.editor.session().unwrap().map,
            before,
            "a refused paint changed the held map"
        );
        // And nothing to undo, because nothing happened.
        assert!(
            refusal(&fixture.post("/api/undo", "")).contains("nothing left to undo"),
            "a refused paint left an undo step behind"
        );
    }

    #[test]
    fn saving_writes_a_new_file_that_parses_with_the_painted_cells_in_it() {
        let mut fixture = Fixture::new("save");
        fixture.open();
        fixture.paint((5, 2, 5, 2), 2, None);
        let output = fixture.dir.join("out.scn");
        let json = fixture.save(&output);
        assert!(json.contains("\"ok\":true"), "{json}");

        let written = fs::read(&output).unwrap();
        assert_eq!(
            number_field(&json, "bytes") as usize,
            written.len(),
            "the reported byte count is not the file's"
        );
        let reparsed = MapAsset::parse(&written).unwrap();
        assert_eq!(reparsed.cell(5, 2).unwrap().tile_index(), 12, "region");
        assert_eq!(reparsed.cell(5, 1).unwrap().tile_index(), 4, "N");
        assert_eq!(reparsed.cell(5, 3).unwrap().tile_index(), 3, "S");
        // Everything two cells out is untouched grass, so the save wrote the session's map and not
        // a wider rewrite.
        assert_eq!(reparsed.cell(5, 0).unwrap().tile_index(), 0);
        assert_eq!(reparsed.cell(2, 2).unwrap().tile_index(), 0);
        // The input is untouched: the loose `map/` directory has no backup.
        assert_eq!(
            fs::read(&fixture.map).unwrap(),
            grass_map(FIXTURE_WIDTH, FIXTURE_HEIGHT)
        );
    }

    #[test]
    fn saving_never_writes_in_place_and_never_over_an_existing_file() {
        let mut fixture = Fixture::new("save-guards");
        fixture.open();
        fixture.paint((5, 2, 5, 2), 2, None);

        // The open map itself, by a path that is spelled differently.
        let alias = fixture.dir.join(".").join("in.scn");
        let same = refusal(&fixture.save(&alias));
        assert!(same.contains("refusing to write to the open map"), "{same}");
        assert_eq!(
            fs::read(&fixture.map).unwrap(),
            grass_map(FIXTURE_WIDTH, FIXTURE_HEIGHT),
            "the open map was overwritten"
        );

        // A file that already exists, whatever it holds.
        let occupied = fixture.dir.join("taken.scn");
        fs::write(&occupied, b"not a map").unwrap();
        let exists = refusal(&fixture.save(&occupied));
        assert!(exists.contains("never writes over an existing file"), "{exists}");
        assert_eq!(fs::read(&occupied).unwrap(), b"not a map");

        // A class change: a world map saved under a combat extension would be drawn through a
        // different tileset entirely.
        let wrong_class = fixture.dir.join("out.smp");
        let class = refusal(&fixture.save(&wrong_class));
        assert!(class.contains("read through different tilesets"), "{class}");
        assert!(!wrong_class.exists(), "a refused save left a file behind");
    }

    #[test]
    fn undo_walks_back_more_than_one_paint_and_stops_at_the_bound() {
        // A 31x5 map, so five single-cell paints four cells apart have rings that never meet --
        // two rings sharing a cell would need a tile with stone on both sides, which this fixture
        // deliberately does not declare.
        let mut fixture = Fixture::with_map("undo", 31, 5);
        let opened = tiles_field(&fixture.open());
        let mut history = vec![opened.clone()];
        // Five separate paints in five separate places, so each one has its own grid to come back
        // to. A one-level undo passes the first step of this and fails the second.
        for rect in [(1, 1, 1, 1), (5, 1, 5, 1), (9, 1, 9, 1), (13, 1, 13, 1), (17, 1, 17, 1)] {
            let json = fixture.paint(rect, 2, None);
            assert!(json.contains("\"ok\":true"), "{json}");
            history.push(
                fixture
                    .editor
                    .session()
                    .unwrap()
                    .map
                    .cells
                    .iter()
                    .map(MapCellTile::tile)
                    .collect(),
            );
        }
        assert_ne!(history[1], history[0], "the fixture painted nothing");
        for expected in history.iter().rev().skip(1) {
            let undone = fixture.post("/api/undo", "");
            assert!(undone.contains("\"ok\":true"), "{undone}");
            assert_eq!(&tiles_field(&undone), expected);
        }
        // Back at the opened state, with nothing left.
        assert!(refusal(&fixture.post("/api/undo", "")).contains("nothing left to undo"));
    }

    #[test]
    fn the_undo_stack_is_bounded_and_says_how_deep_it_is() {
        assert_eq!(UNDO_DEPTH, 32);
        let mut fixture = Fixture::new("undo-bound");
        fixture.open();
        // One more paint than the stack holds. Alternating stone and grass on one cell keeps every
        // step legal on a small fixture and keeps every step a real change.
        for step in 0..=UNDO_DEPTH {
            let terrain = if step % 2 == 0 { 2 } else { 1 };
            let json = fixture.paint((5, 2, 5, 2), terrain, None);
            assert!(json.contains("\"ok\":true"), "step {step}: {json}");
        }
        let depth = fixture.editor.session().unwrap().undo.len();
        assert_eq!(depth, UNDO_DEPTH, "the stack grew past its bound");
        for _ in 0..UNDO_DEPTH {
            let undone = fixture.post("/api/undo", "");
            assert!(undone.contains("\"ok\":true"), "{undone}");
        }
        // The oldest paint is gone from the stack, so the map does not come all the way back --
        // and that is reported rather than silently pretended away.
        assert!(refusal(&fixture.post("/api/undo", "")).contains("nothing left to undo"));
        // 33 paints were made and 32 taken back, so the very first one is still in the map: it
        // fell off the bottom of the bounded stack rather than being silently replayed.
        assert_eq!(
            fixture.editor.session().unwrap().map.cell(5, 2).unwrap().tile_index(),
            12,
            "the paint that fell off the stack should still stand in the map"
        );
    }

    #[test]
    fn the_draw_account_describes_the_map_and_not_the_session_s_history() {
        let mut fixture = Fixture::new("draw-account");
        fixture.open();
        // A 3x3 stone region: the centre has three interchangeable interiors and held none of
        // them, so it is a draw the engine would have made differently.
        let drawn = fixture.paint((4, 1, 6, 3), 2, None);
        assert!(drawn.contains("drawn:1\""), "{drawn}");
        assert_eq!(fixture.editor.session().unwrap().drawn_cells.len(), 1);

        // Paint the same region back to grass. The tileset determines every cell of it, so the map
        // is fully reproducible again -- and the account has to say so. A running total keeps
        // reporting the earlier draw forever, and an honesty mechanism that cries wolf is one
        // people learn to ignore.
        let back = fixture.paint((4, 1, 6, 3), 1, None);
        assert!(back.contains("\"ok\":true"), "{back}");
        assert!(
            fixture.editor.session().unwrap().drawn_cells.is_empty(),
            "a cell repainted to a determined tile is still counted as drawn"
        );
        let json = fixture.save(&fixture.dir.join("clean.scn").clone());
        assert!(json.contains("\"ok\":true"), "{json}");
        assert!(
            !json.contains("drawn among equally valid"),
            "a reproducible file was reported as containing a draw: {json}"
        );

        // Undo the repaint and the draw comes back with the map it belongs to.
        fixture.post("/api/undo", "");
        assert_eq!(fixture.editor.session().unwrap().drawn_cells.len(), 1);
        let json = fixture.save(&fixture.dir.join("dirty.scn").clone());
        assert!(
            json.contains("1 cell in this file still holds a tile drawn"),
            "{json}"
        );
        // And undoing the first paint leaves nothing drawn, because nothing was painted.
        fixture.post("/api/undo", "");
        assert!(fixture.editor.session().unwrap().drawn_cells.is_empty());
    }

    /// A trait so the test can read a cell's tile without depending on field order.
    trait MapCellTile {
        fn tile(&self) -> u32;
    }

    impl MapCellTile for crate::map::MapCell {
        fn tile(&self) -> u32 {
            self.tile_index()
        }
    }

    #[test]
    fn a_combat_map_with_no_binding_is_refused_before_any_tileset_is_read() {
        // `AIBRKS0.SMP` is one of the 168 installed combat maps no gamescript encounter binds to a
        // tileset. The archive here does not exist, so a refusal that names it proves the tileset
        // was never guessed at: a version that defaulted to a shipped tileset would get as far as
        // failing to open the archive, and say so instead.
        let mut notes = Vec::new();
        let unresolved = load_tile_set(
            Path::new("/maps/AIBRKS0.SMP"),
            &TileSetSource::Archive(PathBuf::from("/nonexistent.mpq")),
            &mut notes,
        )
        .unwrap_err();
        assert!(unresolved.contains("will not guess"), "{unresolved}");
        assert!(!unresolved.contains("nonexistent.mpq"), "{unresolved}");

        // Five encounters read `chcave.smp` through five different tilesets. There is no single
        // right answer, so the user is told the set rather than handed the first one.
        let ambiguous = load_tile_set(
            Path::new("/maps/chcave.smp"),
            &TileSetSource::Archive(PathBuf::from("/nonexistent.mpq")),
            &mut notes,
        )
        .unwrap_err();
        assert!(ambiguous.contains("no single answer"), "{ambiguous}");
        assert!(ambiguous.contains("cavelava.til"), "{ambiguous}");

        // A map that *does* resolve gets past resolution and fails on the archive instead, which
        // is what makes the two assertions above mean something.
        let resolved = load_tile_set(
            Path::new("/maps/AIBLDG01.SMP"),
            &TileSetSource::Archive(PathBuf::from("/nonexistent.mpq")),
            &mut notes,
        )
        .unwrap_err();
        assert!(resolved.contains("nonexistent.mpq"), "{resolved}");
    }

    #[test]
    fn a_malformed_percent_escape_is_literal_text_and_not_a_panic() {
        // **Bytes, not a string literal.** A `&str` in the test source has already been validated
        // as UTF-8, so a decoder that panics on a boundary would still pass. These are the exact
        // bytes a socket delivers: `%` followed by the three bytes of a euro sign, where the old
        // decoder sliced `value[1..3]` straight into the middle of the character.
        let raw = b"x0=%\xe2\x82\xac&y0=1&x1=2&y1=2&terrain=1";
        let source = String::from_utf8(raw.to_vec()).expect("the fixture is valid UTF-8 overall");
        let fields = form_fields(&source);
        assert_eq!(fields.get("x0").map(String::as_str), Some("%\u{20ac}"));
        assert_eq!(fields.get("terrain").map(String::as_str), Some("1"));

        // A truncated escape at the very end, and one whose digits are not hex.
        assert_eq!(form_fields("path=%").get("path").map(String::as_str), Some("%"));
        assert_eq!(form_fields("path=%4").get("path").map(String::as_str), Some("%4"));
        assert_eq!(form_fields("path=%zz").get("path").map(String::as_str), Some("%zz"));
        // And the well-formed case still decodes, including lower-case digits.
        assert_eq!(form_fields("path=%2f%2F").get("path").map(String::as_str), Some("//"));
    }

    #[test]
    fn a_panicking_handler_costs_one_request_and_not_the_session() {
        // The session model exists so unsaved paints accumulate. A single-threaded server that
        // dies on any handler panic throws all of them away with nothing on disk, so the loop --
        // not just the one decoder that panicked -- has to survive.
        let dir = scratch_dir("panic");
        let map = dir.join("in.scn");
        fs::write(&map, grass_map(FIXTURE_WIDTH, FIXTURE_HEIGHT)).unwrap();
        fs::write(dir.join("fixture.til"), FIXTURE_TILESET).unwrap();
        fs::write(
            dir.join("fixture.lbm"),
            fixture_atlas(FIXTURE_COLUMNS, FIXTURE_ROWS),
        )
        .unwrap();
        let source = TileSetSource::Loose {
            definition: dir.join("fixture.til"),
            atlas: dir.join("fixture.lbm"),
        };
        let (server, address) = listen(0).unwrap();
        let port = address.port();
        thread::spawn(move || run(&server, source, port));

        let post = |path: &str, form: &[u8]| {
            let mut request = format!(
                "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\
                 Content-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n",
                form.len()
            )
            .into_bytes();
            request.extend_from_slice(form);
            raw_bytes(address, &request)
        };

        let opened = post(
            "/api/open",
            format!("path={}", encode(&map.display().to_string())).as_bytes(),
        )
        .1;
        assert!(opened.contains("\"ok\":true"), "{opened}");
        let token = {
            let start = opened.find("\"token\":\"").expect("token") + "\"token\":\"".len();
            opened[start..start + opened[start..].find('"').unwrap()].to_owned()
        };
        let painted = post(
            "/api/paint",
            format!("x0=5&y0=2&x1=5&y1=2&terrain=2&token={token}").as_bytes(),
        )
        .1;
        assert!(painted.contains("\"ok\":true"), "{painted}");

        // The bytes that used to end the process. They are now ordinary text, which is the
        // decoder's fix -- see the test above. What this one is for is the *loop*.
        let mut hostile = Vec::from(&b"x0=%"[..]);
        hostile.extend_from_slice("\u{20ac}".as_bytes());
        hostile.extend_from_slice(format!("&y0=1&x1=2&y1=2&terrain=1&token={token}").as_bytes());
        let (status, _) = post("/api/paint", &hostile);
        assert!(!status.is_empty(), "the server did not answer at all: {status}");

        // Still alive, still holding the paint, and it saves.
        let output = dir.join("out.scn");
        let saved = post(
            "/api/save",
            format!("path={}&token={token}", encode(&output.display().to_string())).as_bytes(),
        )
        .1;
        assert!(saved.contains("\"ok\":true"), "the session was lost: {saved}");
        assert_eq!(
            MapAsset::parse(&fs::read(&output).unwrap())
                .unwrap()
                .cell(5, 2)
                .unwrap()
                .tile_index(),
            12,
            "the paint made before the malformed request did not survive it"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_handler_that_panics_costs_one_request_and_not_the_loop() {
        // **The decoder's fix cannot stand in for this.** With the one known panic gone there is
        // nothing in the editor left to panic on demand, so a test that only sends the malformed
        // bytes proves the decoder and says nothing about whether the loop survives the *next*
        // bug. This drives the real `run_with` over a real socket with a handler that panics, and
        // checks the connection after it is still answered.
        let (server, address) = listen(0).unwrap();
        let port = address.port();
        thread::spawn(move || {
            let mut answered = 0_usize;
            run_with(&server, move |request| {
                if request.target == "/boom" {
                    panic!("a handler bug");
                }
                answered += 1;
                HttpResponse::json(format!("{{\"ok\":true,\"answered\":{answered}}}"))
            });
        });

        let get = |path: &str| {
            raw_request(
                address,
                &format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"),
            )
        };
        assert!(get("/first").1.contains("\"answered\":1"));

        let (status, body) = get("/boom");
        assert!(status.contains("500"), "{status}");
        assert!(
            body.contains("still running and the open map is untouched"),
            "a panic must be reported to the page, not dropped: {body}"
        );

        // The loop is still running and its state -- the counter here, a held map in the real
        // thing -- survived.
        assert!(
            get("/after").1.contains("\"answered\":2"),
            "the loop did not survive a handler panic"
        );
    }

    #[test]
    fn a_request_from_another_site_is_refused_whatever_it_asks_for() {
        let mut fixture = Fixture::new("cross-origin");
        fixture.open();
        let token = fixture.token.clone();

        let hostile = |host: Option<&'static str>, origin: Option<&'static str>| {
            HttpRequest {
                method: "POST",
                target: "/api/open",
                body: "path=/etc/hosts",
                host,
                origin,
            }
        };
        // A page on another site: correct Host, foreign Origin.
        let foreign = fixture
            .editor
            .handle(&hostile(Some("127.0.0.1:8731"), Some("https://evil.example")));
        assert_eq!(foreign.status, 403);
        assert!(
            String::from_utf8(foreign.body).unwrap().contains("evil.example"),
            "the refusal must name what it refused"
        );

        // DNS rebinding: the page's own origin, because after rebinding it *is* same-origin. Only
        // the Host header sees this, which is why an Origin-only check would not be enough.
        let rebound = fixture.editor.handle(&hostile(
            Some("evil.example:8731"),
            Some("http://evil.example:8731"),
        ));
        assert_eq!(rebound.status, 403);

        // A loopback name on somebody else's port is still somebody else.
        assert_eq!(
            fixture
                .editor
                .handle(&hostile(Some("127.0.0.1:9999"), None))
                .status,
            403
        );
        // No Host at all.
        assert_eq!(fixture.editor.handle(&hostile(None, None)).status, 403);

        // Nothing got through: the fixture's own map is still the open one.
        let held = fixture.editor.session().unwrap().path.clone();
        assert_eq!(held, fixture.map);

        // And the editor's own page is not caught by any of it.
        for origin in ["http://127.0.0.1:8731", "http://localhost:8731", "http://[::1]:8731"] {
            for host in ["127.0.0.1:8731", "localhost:8731", "[::1]:8731"] {
                let response = fixture.editor.handle(&HttpRequest {
                    method: "POST",
                    target: "/api/undo",
                    body: &format!("token={token}"),
                    host: Some(host),
                    origin: Some(origin),
                });
                assert_ne!(response.status, 403, "{host} / {origin} was refused");
            }
        }
    }

    #[test]
    fn a_stale_tab_s_handle_is_refused_rather_than_painting_into_the_wrong_map() {
        let mut fixture = Fixture::new("handle");
        fixture.open();
        let first = fixture.token.clone();
        // A second tab opens something else. The editor holds one map per process, so the first
        // tab's canvas now shows a map the server is not holding.
        let second_map = fixture.dir.join("other.scn");
        fs::write(&second_map, grass_map(FIXTURE_WIDTH, FIXTURE_HEIGHT)).unwrap();
        fixture.open_path(&second_map.display().to_string());
        assert_ne!(fixture.token, first, "a new open must hand out a new handle");

        let stale = fixture.editor.handle(&own_page(
            "POST",
            "/api/paint",
            &format!("x0=5&y0=2&x1=5&y1=2&terrain=2&token={first}"),
        ));
        let stale = refusal(&String::from_utf8(stale.body).unwrap());
        assert!(stale.contains("no longer the open one"), "{stale}");
        assert!(stale.contains("other.scn"), "{stale}");
        // Nothing was painted into the second map at the first tab's coordinates.
        assert_eq!(
            fixture.editor.session().unwrap().map.cell(5, 2).unwrap().tile_index(),
            0
        );
    }

    #[test]
    fn a_refused_open_keeps_the_map_it_was_already_holding_and_says_which() {
        let mut fixture = Fixture::new("open-refused");
        fixture.open();
        fixture.paint((5, 2, 5, 2), 2, None);
        let token = fixture.token.clone();

        let refused = fixture.open_path("/definitely/not/here.scn");
        assert!(refused.contains("\"ok\":false"), "{refused}");
        // The client is told what is still held, so it cannot report "No map open." over a live
        // session and then write a file the user believes does not exist.
        assert!(
            refused.contains(&format!("\"holding\":{}", json_string(&fixture.map.display().to_string()))),
            "{refused}"
        );
        // A refused open must not hand out a handle, or the stale-tab check would pass on it.
        assert!(!refused.contains("\"token\""), "{refused}");
        assert_eq!(fixture.token, token, "the handle changed on a refused open");

        // The held map is still the painted one, unchanged.
        assert_eq!(
            fixture.editor.session().unwrap().map.cell(5, 2).unwrap().tile_index(),
            12
        );
        // And when nothing is open, `holding` is null rather than absent.
        let mut empty = Fixture::new("open-refused-empty");
        let first = empty.open_path("/definitely/not/here.scn");
        assert!(first.contains("\"holding\":null"), "{first}");
    }

    #[test]
    fn a_body_larger_than_the_cap_cannot_make_the_editor_allocate_without_limit() {
        // The cap is what makes this bounded; the assertion is that the limit is the one the
        // constant names, because an off-by-a-factor here is invisible otherwise.
        assert_eq!(MAX_BODY_BYTES, 65_536);
        let dir = scratch_dir("body-cap");
        fs::write(dir.join("fixture.til"), FIXTURE_TILESET).unwrap();
        fs::write(
            dir.join("fixture.lbm"),
            fixture_atlas(FIXTURE_COLUMNS, FIXTURE_ROWS),
        )
        .unwrap();
        let source = TileSetSource::Loose {
            definition: dir.join("fixture.til"),
            atlas: dir.join("fixture.lbm"),
        };
        let (server, address) = listen(0).unwrap();
        let port = address.port();
        thread::spawn(move || run(&server, source, port));

        let oversize = "x".repeat(200_000);
        let body = format!("path={oversize}");
        let (status, answer) = raw_request(
            address,
            &format!(
                "POST /api/open HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\
                 Content-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            ),
        );
        // Truncated at the cap, so it is answered as an ordinary refusal rather than read whole.
        assert!(status.contains("200"), "{status}");
        assert!(answer.contains("\"ok\":false"), "{answer}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn listing_a_directory_reports_its_map_files_and_never_leaves_it() {
        let mut fixture = Fixture::new("listing");
        let dir = fixture.dir.clone();
        fs::write(dir.join("Beta.SCN"), b"x").unwrap();
        fs::write(dir.join("alpha.scn"), b"xx").unwrap();
        fs::write(dir.join("battle.smp"), b"xxx").unwrap();
        fs::write(dir.join("notes.txt"), b"xxxx").unwrap();
        fs::create_dir(dir.join("subdir")).unwrap();
        fs::write(dir.join("subdir").join("hidden.scn"), b"xxxxx").unwrap();

        let listed = String::from_utf8(
            fixture
                .editor
                .handle(&own_page(
                    "GET",
                    &format!("/api/list?dir={}", encode(&dir.display().to_string())),
                    "",
                ))
                .body,
        )
        .unwrap();
        assert!(listed.contains("\"ok\":true"), "{listed}");
        // Sorted case-insensitively, so `Beta.SCN` sits between `alpha` and `battle` rather than
        // ahead of both. The installed corpus is split across cases -- 172 `.smp` and 165 `.SMP`.
        let names: Vec<&str> = listed
            .match_indices("\"name\":\"")
            .map(|(at, needle)| {
                let rest = &listed[at + needle.len()..];
                &rest[..rest.find('"').unwrap()]
            })
            .collect();
        assert_eq!(names, vec!["alpha.scn", "battle.smp", "Beta.SCN", "in.scn"]);
        // A directory is not listed, so there is nothing to descend into, and the file inside it
        // is not reachable through this endpoint at all.
        assert!(!listed.contains("subdir"), "{listed}");
        assert!(!listed.contains("hidden.scn"), "{listed}");
        // A file that is not a map by extension is counted, not listed.
        assert!(!listed.contains("notes.txt"), "{listed}");
        // `notes.txt` plus the fixture's own `.til` and `.lbm`.
        assert!(listed.contains("3 other files in this directory are not a map by extension"), "{listed}");

        let missing = refusal(
            &String::from_utf8(
                fixture
                    .editor
                    .handle(&own_page("GET", "/api/list?dir=/definitely/not/here", ""))
                    .body,
            )
            .unwrap(),
        );
        assert!(missing.contains("could not list"), "{missing}");
    }

    #[test]
    fn a_listing_follows_a_symlinked_directory_rather_than_resolving_it_away() {
        // The obvious workaround for a 180-character path is a symlink -- the user is already
        // working through one. Canonicalising would make the listed names belong to a path they
        // did not type, and the save they then make would land somewhere they did not choose.
        let mut fixture = Fixture::new("listing-symlink");
        let real = fixture.dir.join("real");
        fs::create_dir(&real).unwrap();
        fs::write(real.join("through.scn"), b"x").unwrap();
        let link = fixture.dir.join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let listed = String::from_utf8(
            fixture
                .editor
                .handle(&own_page(
                    "GET",
                    &format!("/api/list?dir={}", encode(&link.display().to_string())),
                    "",
                ))
                .body,
        )
        .unwrap();
        assert!(listed.contains("through.scn"), "{listed}");
        assert!(
            listed.contains(&json_string(&link.display().to_string())),
            "the listing reported a path the user did not ask for: {listed}"
        );
    }

    #[test]
    fn saving_by_directory_and_filename_cannot_leave_the_directory() {
        let mut fixture = Fixture::new("save-by-name");
        fixture.open();
        fixture.paint((5, 2, 5, 2), 2, None);
        // A directory *inside* the fixture's own scratch tree, so the place an escape would land
        // is ours and is cleaned up. Pointing this at the shared temp directory made the test pass
        // or fail on whatever a previous run had left there.
        let dir = fixture.dir.join("maps");
        fs::create_dir(&dir).unwrap();
        let token = fixture.token.clone();

        let save = |fixture: &mut Fixture, form: String| {
            String::from_utf8(
                fixture
                    .editor
                    .handle(&own_page("POST", "/api/save", &format!("{form}&token={token}")))
                    .body,
            )
            .unwrap()
        };

        let written = save(
            &mut fixture,
            format!("dir={}&name=out.scn", encode(&dir.display().to_string())),
        );
        assert!(written.contains("\"ok\":true"), "{written}");
        assert!(dir.join("out.scn").is_file());

        // A typed name that climbs. `create_new` would still refuse an existing file, but the user
        // asked to write in the directory they chose and the tool writes there or refuses.
        for escape in ["../escaped.scn", "sub/escaped.scn", "/tmp/escaped.scn", ".."] {
            let refused = refusal(&save(
                &mut fixture,
                format!(
                    "dir={}&name={}",
                    encode(&dir.display().to_string()),
                    encode(escape)
                ),
            ));
            assert!(refused.contains("not a plain filename"), "{escape}: {refused}");
        }
        assert!(
            !fixture.dir.join("escaped.scn").exists(),
            "a typed name climbed out of the chosen directory"
        );
        assert!(!fixture.dir.join("maps").join("sub").exists());

        // The picker is not a way round the create-new rule.
        let again = refusal(&save(
            &mut fixture,
            format!("dir={}&name=out.scn", encode(&dir.display().to_string())),
        ));
        assert!(again.contains("never writes over an existing file"), "{again}");
    }

    #[test]
    fn the_editor_suggests_the_maps_directory_beside_the_archive_only_when_it_exists() {
        let dir = scratch_dir("config");
        let archive = dir.join("pic.mpq");
        fs::write(&archive, b"not really an archive").unwrap();
        let mut editor = Editor::new(TileSetSource::Archive(archive.clone()), FIXTURE_PORT);
        let without = String::from_utf8(editor.handle(&own_page("GET", "/api/config", "")).body)
            .unwrap();
        assert!(without.contains("\"mapsDirectory\":null"), "{without}");

        fs::create_dir(dir.join("map")).unwrap();
        let with = String::from_utf8(editor.handle(&own_page("GET", "/api/config", "")).body)
            .unwrap();
        assert!(
            with.contains(&json_string(&dir.join("map").display().to_string())),
            "{with}"
        );
        assert!(with.contains(&format!("\"undoDepth\":{UNDO_DEPTH}")), "{with}");

        // The loose form names no archive, so there is nothing to derive and nothing is invented.
        let mut loose = Editor::new(
            TileSetSource::Loose {
                definition: dir.join("a.til"),
                atlas: dir.join("a.lbm"),
            },
            FIXTURE_PORT,
        );
        let none = String::from_utf8(loose.handle(&own_page("GET", "/api/config", "")).body).unwrap();
        assert!(none.contains("\"mapsDirectory\":null"), "{none}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_chosen_path_comes_back_and_a_cancel_is_not_an_error() {
        let mut fixture = Fixture::new("picker");
        let seen: std::sync::Arc<std::sync::Mutex<Vec<PickRequest>>> = Default::default();

        // A path, the ordinary case.
        let recorder = std::sync::Arc::clone(&seen);
        fixture.editor.set_picker(Box::new(move |request| {
            recorder.lock().unwrap().push(request.clone());
            PickOutcome::Chosen(PathBuf::from("/chosen/maps"))
        }));
        let chosen = String::from_utf8(
            fixture
                .editor
                .handle(&own_page("POST", "/api/pick-directory", "dir=/nowhere"))
                .body,
        )
        .unwrap();
        assert!(chosen.contains("\"ok\":true"), "{chosen}");
        assert!(chosen.contains("\"cancelled\":false"), "{chosen}");
        assert!(chosen.contains("\"path\":\"/chosen/maps\""), "{chosen}");
        // A directory that does not exist is not offered to the dialog as a starting point: the
        // macOS chooser errors on one rather than ignoring it.
        assert_eq!(seen.lock().unwrap()[0].start_in, None);
        assert_eq!(seen.lock().unwrap()[0].kind, PickKind::Directory);

        // A real directory is passed through, and the save form carries the filename too.
        let dir = fixture.dir.display().to_string();
        let saved = String::from_utf8(
            fixture
                .editor
                .handle(&own_page(
                    "POST",
                    "/api/pick-save",
                    &format!("dir={}&name=out.scn", encode(&dir)),
                ))
                .body,
        )
        .unwrap();
        assert!(saved.contains("\"ok\":true"), "{saved}");
        let request = seen.lock().unwrap()[1].clone();
        assert_eq!(request.kind, PickKind::SaveFile);
        assert_eq!(request.start_in, Some(fixture.dir.clone()));
        assert_eq!(request.default_name.as_deref(), Some("out.scn"));

        // **Cancelling is an answer, not a refusal.** `"ok":true` with nothing chosen, so the page
        // can leave the field exactly as it was and say nothing.
        fixture
            .editor
            .set_picker(Box::new(|_| PickOutcome::Cancelled));
        let cancelled = String::from_utf8(
            fixture
                .editor
                .handle(&own_page("POST", "/api/pick-directory", ""))
                .body,
        )
        .unwrap();
        assert!(cancelled.contains("\"ok\":true"), "{cancelled}");
        assert!(cancelled.contains("\"cancelled\":true"), "{cancelled}");
        assert!(cancelled.contains("\"path\":null"), "{cancelled}");
        assert!(!cancelled.contains("refusal"), "a cancel must not read as a failure: {cancelled}");

        // No dialog to show: a refusal that points at the field that still works.
        fixture.editor.set_picker(Box::new(|_| {
            PickOutcome::Unavailable("there is no desktop session".to_owned())
        }));
        let unavailable = refusal(
            &String::from_utf8(
                fixture
                    .editor
                    .handle(&own_page("POST", "/api/pick-directory", ""))
                    .body,
            )
            .unwrap(),
        );
        assert!(unavailable.contains("no desktop session"), "{unavailable}");
        assert!(unavailable.contains("Type the path into the field"), "{unavailable}");
    }

    #[test]
    fn the_file_dialog_is_a_post_and_gets_the_same_guards_as_everything_else() {
        let mut fixture = Fixture::new("picker-guards");
        fixture.editor.set_picker(Box::new(|_| {
            panic!("a guarded request must never reach the dialog")
        }));
        // A cross-origin page must not be able to make the user's machine pop a file dialog.
        for target in ["/api/pick-directory", "/api/pick-save"] {
            let foreign = fixture.editor.handle(&HttpRequest {
                method: "POST",
                target,
                body: "",
                host: Some("127.0.0.1:8731"),
                origin: Some("https://evil.example"),
            });
            assert_eq!(foreign.status, 403, "{target}");
            let rebound = fixture.editor.handle(&HttpRequest {
                method: "POST",
                target,
                body: "",
                host: Some("evil.example:8731"),
                origin: None,
            });
            assert_eq!(rebound.status, 403, "{target}");
            // And not reachable by GET, which is what an `<img>` or a redirect could produce.
            assert_eq!(fixture.editor.handle(&own_page("GET", target, "")).status, 404);
        }
    }

    #[test]
    fn a_picked_path_is_validated_exactly_like_a_typed_one() {
        let mut fixture = Fixture::new("picker-validation");
        fixture.open();
        fixture.paint((5, 2, 5, 2), 2, None);
        let token = fixture.token.clone();
        let taken = fixture.dir.join("taken.scn");
        fs::write(&taken, b"already here").unwrap();

        // The native save dialog asks its own "replace?" question and hands back an existing path
        // when the user says yes. We refuse it anyway, and say why -- the map directory has no
        // backup and this tool never writes in place, whatever the OS offered.
        fixture.editor.set_picker(Box::new({
            let taken = taken.clone();
            move |_| PickOutcome::Chosen(taken.clone())
        }));
        let picked = String::from_utf8(
            fixture
                .editor
                .handle(&own_page("POST", "/api/pick-save", ""))
                .body,
        )
        .unwrap();
        assert!(picked.contains("\"ok\":true"), "{picked}");

        let refused = refusal(&String::from_utf8(
            fixture
                .editor
                .handle(&own_page(
                    "POST",
                    "/api/save",
                    &format!("path={}&token={token}", encode(&taken.display().to_string())),
                ))
                .body,
        )
        .unwrap());
        assert!(refused.contains("never writes over an existing file"), "{refused}");
        assert!(refused.contains("save dialog"), "{refused}");
        assert_eq!(fs::read(&taken).unwrap(), b"already here");

        // And a picked path that would rename the map's class is refused like a typed one.
        let combat = fixture.dir.join("picked.smp");
        let class = refusal(&String::from_utf8(
            fixture
                .editor
                .handle(&own_page(
                    "POST",
                    "/api/save",
                    &format!("path={}&token={token}", encode(&combat.display().to_string())),
                ))
                .body,
        )
        .unwrap());
        assert!(class.contains("read through different tilesets"), "{class}");
        assert!(!combat.exists());
    }

    #[test]
    fn a_finished_dialog_is_read_as_a_path_a_cancel_or_a_failure() {
        // The three outcomes, from the bytes `osascript` actually produces. This is the part that
        // decides whether a user who dismissed a dialog sees a refusal they did not earn.
        assert_eq!(
            classify_pick(true, b"/Users/someone/English/map/\n", ""),
            PickOutcome::Chosen(PathBuf::from("/Users/someone/English/map/"))
        );
        // The cancel is matched on AppleScript's error **number**, because the text is localised
        // and the number is not. A French or Japanese system says something else entirely.
        assert_eq!(
            classify_pick(false, b"", "1:1: execution error: User canceled. (-128)\n"),
            PickOutcome::Cancelled
        );
        assert_eq!(
            classify_pick(false, b"", "execution error: erreur inconnue. (-128)\n"),
            PickOutcome::Cancelled
        );
        // A genuine failure keeps its reason.
        let broken = classify_pick(
            false,
            b"",
            "execution error: Application isn't running. (-600)\n",
        );
        match broken {
            PickOutcome::Unavailable(reason) => assert!(reason.contains("-600"), "{reason}"),
            other => panic!("a failure was read as {other:?}"),
        }
        // Success with nothing in it is not a path.
        assert!(matches!(
            classify_pick(true, b"  \n", ""),
            PickOutcome::Unavailable(_)
        ));
    }

    #[test]
    fn a_dialog_that_never_answers_is_killed_rather_than_hanging_the_editor() {
        // The request loop is single-threaded, so a child blocked on a window that will never be
        // drawn freezes the whole editor. No GUI is involved here: `sleep` stands in for the
        // dialog, which is the only part of this that a headless test can exercise honestly.
        let mut command = Command::new("/bin/sleep");
        command.arg("30");
        let started = Instant::now();
        let outcome = run_picker(command, Duration::from_millis(200));
        let elapsed = started.elapsed();
        match outcome {
            PickOutcome::Unavailable(reason) => {
                assert!(reason.contains("did not answer"), "{reason}");
                assert!(reason.contains("stopped"), "{reason}");
            }
            other => panic!("a hung dialog was read as {other:?}"),
        }
        assert!(
            elapsed < Duration::from_secs(5),
            "the timeout did not fire: {elapsed:?}"
        );
        assert_eq!(PICK_TIMEOUT, Duration::from_secs(120));

        // A program that is not there at all is unavailable, not a panic: this is the shape of a
        // machine with no `osascript`.
        match run_picker(
            Command::new("/definitely/not/a/program"),
            Duration::from_millis(200),
        ) {
            PickOutcome::Unavailable(reason) => {
                assert!(reason.contains("could not start"), "{reason}")
            }
            other => panic!("a missing dialog program was read as {other:?}"),
        }
    }

    /// The script's arguments, after the `-e` and the script text itself.
    #[cfg(target_os = "macos")]
    fn script_arguments(request: &PickRequest) -> Vec<String> {
        pick_command(request)
            .get_args()
            .skip(2)
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect()
    }

    /// Measured against `osascript` on 2026-09-17, because this is where the first version was
    /// wrong: `osascript -e SCRIPT - a b` does **not** consume the `-`. It arrives as
    /// `item 1 of argv`, so the prompt became item 2 and the directory item 3, and the dialog
    /// would have been titled `-`. A `sh -c` would have hidden this behind a quoting problem
    /// instead; there is none here, and the arguments are the whole interface.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_dialog_script_gets_its_strings_as_argv_in_the_order_it_reads_them() {
        let directory = PickRequest {
            kind: PickKind::Directory,
            start_in: Some(PathBuf::from("/Program Files (x86)/map")),
            default_name: None,
        };
        assert_eq!(
            script_arguments(&directory),
            vec![
                "Choose the directory your maps are in".to_owned(),
                "/Program Files (x86)/map".to_owned(),
            ],
            "the script reads its prompt from `item 1 of argv`"
        );
        assert_eq!(
            script_arguments(&PickRequest { start_in: None, ..directory.clone() }),
            vec!["Choose the directory your maps are in".to_owned()]
        );

        // Save takes prompt, then name, then location -- the order the script indexes them in.
        let save = PickRequest {
            kind: PickKind::SaveFile,
            start_in: Some(PathBuf::from("/maps")),
            default_name: Some("URAK-edited.scn".to_owned()),
        };
        assert_eq!(
            script_arguments(&save),
            vec![
                "Save the edited map as a new file".to_owned(),
                "URAK-edited.scn".to_owned(),
                "/maps".to_owned(),
            ]
        );
        // With no name typed there is still a name, because `choose file name` needs one.
        assert_eq!(
            script_arguments(&PickRequest { default_name: None, ..save })[1],
            "edited.scn"
        );

        // And the two script bodies really do index the arguments this way, so the assertions
        // above are about the pair and not about one side of it.
        assert!(CHOOSE_FOLDER_IN.contains("prompt (item 1 of argv)"));
        assert!(CHOOSE_FOLDER_IN.contains("POSIX file (item 2 of argv)"));
        assert!(CHOOSE_SAVE_NAME_IN.contains("default name (item 2 of argv)"));
        assert!(CHOOSE_SAVE_NAME_IN.contains("POSIX file (item 3 of argv)"));
        // No separator between the script and its arguments: that is the bug this test exists for.
        assert!(
            !pick_command(&directory)
                .get_args()
                .any(|argument| argument == "-"),
            "a `-` after -e is passed through as argv, not consumed"
        );
    }

    /// Send raw bytes and return the status line and body.
    fn raw_bytes(address: SocketAddr, request: &[u8]) -> (String, String) {
        let mut stream = TcpStream::connect(address).unwrap();
        stream.write_all(request).unwrap();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).unwrap();
        let text = String::from_utf8_lossy(&raw).into_owned();
        let (head, body) = text.split_once("\r\n\r\n").unwrap_or((text.as_str(), ""));
        (head.lines().next().unwrap_or_default().to_owned(), body.to_owned())
    }

    /// Send one raw HTTP/1.1 request and return the status line and body.
    fn raw_request(address: SocketAddr, request: &str) -> (String, String) {
        let mut stream = TcpStream::connect(address).unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).unwrap();
        let text = String::from_utf8_lossy(&raw).into_owned();
        let (head, body) = text.split_once("\r\n\r\n").unwrap_or((text.as_str(), ""));
        let status = head.lines().next().unwrap_or_default().to_owned();
        (status, body.to_owned())
    }

    /// The whole loop over a real socket: bind, route, read a body, answer, and write a file.
    ///
    /// Everything else in this module calls [`Editor::handle`] directly, which cannot notice a
    /// listener bound to the wrong interface, a method never extracted, or a POST body never read.
    #[test]
    fn the_server_binds_loopback_only_and_answers_over_a_real_socket() {
        let dir = scratch_dir("socket");
        let map = dir.join("in.scn");
        fs::write(&map, grass_map(FIXTURE_WIDTH, FIXTURE_HEIGHT)).unwrap();
        fs::write(dir.join("fixture.til"), FIXTURE_TILESET).unwrap();
        fs::write(
            dir.join("fixture.lbm"),
            fixture_atlas(FIXTURE_COLUMNS, FIXTURE_ROWS),
        )
        .unwrap();
        let source = TileSetSource::Loose {
            definition: dir.join("fixture.til"),
            atlas: dir.join("fixture.lbm"),
        };

        let (server, address) = listen(0).unwrap();
        assert!(
            address.ip().is_loopback(),
            "the editor reads and writes local files and must not be reachable off-host, but it \
             bound {address}"
        );
        let port = address.port();
        thread::spawn(move || run(&server, source, port));

        let (status, body) = raw_request(
            address,
            &{
                let body = format!("path={}", encode(&map.display().to_string()));
                format!(
                    "POST /api/open HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\
                     Content-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n{body}",
                    address.port(),
                    body.len()
                )
            },
        );
        assert!(status.contains("200"), "{status}");
        assert!(body.contains("\"ok\":true"), "{body}");

        let token = {
            let start = body.find("\"token\":\"").expect("token") + "\"token\":\"".len();
            body[start..start + body[start..].find('"').unwrap()].to_owned()
        };
        let form_post = |path: &str, form: &str| {
            raw_request(
                address,
                &format!(
                    "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\
                     Content-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n{form}",
                    form.len()
                ),
            )
        };

        let (status, body) = form_post("/api/paint", &format!("x0=5&y0=2&x1=5&y1=2&terrain=2&token={token}"));
        assert!(status.contains("200"), "{status}");
        assert!(body.contains("\"tile\":12"), "the POST body never reached the handler: {body}");

        let output = dir.join("out.scn");
        let (_, body) = form_post(
            "/api/save",
            &format!("path={}&token={token}", encode(&output.display().to_string())),
        );
        assert!(body.contains("\"ok\":true"), "{body}");
        assert_eq!(
            MapAsset::parse(&fs::read(&output).unwrap())
                .unwrap()
                .cell(5, 2)
                .unwrap()
                .tile_index(),
            12
        );

        let (status, _) = raw_request(
            address,
            &format!("GET /nope HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"),
        );
        assert!(status.contains("404"), "{status}");
        let _ = fs::remove_dir_all(&dir);
    }
}
