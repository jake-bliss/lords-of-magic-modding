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

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

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
    /// The map as it was before the last paint. One level, by design for v1.
    undo: Option<MapAsset>,
    pub tile_set: LoadedTileSet,
    /// Cells this tool drew among equally valid tiles, across the whole session.
    ///
    /// Accumulated rather than per-paint, because the number that matters when a file is written
    /// is how much of *the file* is a legal choice rather than the engine's.
    drawn_cells: usize,
    paints: usize,
    /// The drawn-cell count of the paint `undo` would roll back.
    ///
    /// Held so that undoing one paint subtracts one paint's worth of unreproducibility rather than
    /// resetting the account. An earlier version zeroed it, which made a session of 197 paints
    /// report a clean file after undoing the last one -- understating exactly the thing the tool
    /// exists to be honest about.
    undo_drawn_cells: usize,
}

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

/// The whole editor: a tileset source, and at most one open map.
pub struct Editor {
    source: TileSetSource,
    session: Option<EditorSession>,
}

impl Editor {
    pub fn new(source: TileSetSource) -> Self {
        Self {
            source,
            session: None,
        }
    }

    /// The open session, for tests and for [`serve`]'s startup banner.
    pub fn session(&self) -> Option<&EditorSession> {
        self.session.as_ref()
    }

    /// Route one request. `target` is the raw request target, path and query together.
    pub fn handle(&mut self, method: &str, target: &str, body: &str) -> HttpResponse {
        let (path, query) = match target.split_once('?') {
            Some((path, query)) => (path, query),
            None => (target, ""),
        };
        match (method, path) {
            ("GET", "/") | ("GET", "/index.html") => {
                HttpResponse::text(200, "text/html; charset=utf-8", INDEX_HTML)
            }
            ("GET", "/app.js") => {
                HttpResponse::text(200, "text/javascript; charset=utf-8", APP_JS)
            }
            ("GET", "/style.css") => HttpResponse::text(200, "text/css; charset=utf-8", STYLE_CSS),
            ("GET", "/api/open") => self.open(&form_fields(query)),
            ("GET", "/api/atlas.png") => self.atlas_png(),
            ("POST", "/api/paint") => self.paint(&form_fields(body)),
            ("POST", "/api/undo") => self.undo(),
            ("POST", "/api/save") => self.save(&form_fields(body)),
            _ => HttpResponse {
                status: 404,
                content_type: "application/json; charset=utf-8",
                body: format!(
                    "{{\"ok\":false,\"refusal\":{},\"notes\":[]}}",
                    json_string(&format!("no endpoint {method} {path}"))
                )
                .into_bytes(),
            },
        }
    }

    fn open(&mut self, fields: &BTreeMap<String, String>) -> HttpResponse {
        let mut notes = Vec::new();
        let Some(path) = fields.get("path").filter(|value| !value.is_empty()) else {
            return HttpResponse::refusal("no map path was given", &notes);
        };
        let path = PathBuf::from(path);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                return HttpResponse::refusal(
                    &format!("could not read {}: {error}", path.display()),
                    &notes,
                );
            }
        };
        let map = match MapAsset::parse(&bytes) {
            Ok(map) => map,
            Err(error) => {
                return HttpResponse::refusal(
                    &format!("{} does not parse as a map: {error}", path.display()),
                    &notes,
                );
            }
        };
        let Some(class) = MapClass::from_path(&path) else {
            return HttpResponse::refusal(
                &format!(
                    "{} parses as a map but its extension is not one the corpus classifies, so \
                     which tileset the engine reads it through is unknown; the known classes are \
                     .smp (combat) and .scn/.lgd/.map (world)",
                    path.display()
                ),
                &notes,
            );
        };
        let tile_set = match load_tile_set(&path, &self.source, &mut notes) {
            Ok(tile_set) => tile_set,
            Err(reason) => return HttpResponse::refusal(&reason, &notes),
        };

        let body = open_json(&path, class, &map, &tile_set, &notes);
        self.session = Some(EditorSession {
            path,
            map,
            undo: None,
            tile_set,
            drawn_cells: 0,
            paints: 0,
            undo_drawn_cells: 0,
        });
        HttpResponse::json(body)
    }

    fn atlas_png(&self) -> HttpResponse {
        let Some(session) = self.session.as_ref() else {
            return HttpResponse::refusal("no map is open", &[]);
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
        let Some(session) = self.session.as_mut() else {
            return HttpResponse::refusal("no map is open", &[]);
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

        // Planned before anything is touched, so a refusal cannot leave the session holding a
        // half-painted map. `paint_terrain` plans first too; doing it here as well is what lets the
        // snapshot be taken only once the paint is known to be legal.
        let plan = match session
            .map
            .plan_terrain_paint(rect, terrain_type, &session.tile_set.definition, selector)
        {
            Ok(plan) => plan,
            Err(refusal) => return HttpResponse::refusal(&refusal.to_string(), &[]),
        };
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
        debug_assert_eq!(plan, paint.plan);

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
        session.drawn_cells += drawn;
        session.paints += 1;
        session.undo_drawn_cells = drawn;
        session.undo = Some(before);

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
            "{{\"ok\":true,\"cells\":{cells},\"summary\":{},\"notes\":{}}}",
            json_string(&summary),
            json_strings(&notes)
        ))
    }

    fn undo(&mut self) -> HttpResponse {
        let Some(session) = self.session.as_mut() else {
            return HttpResponse::refusal("no map is open", &[]);
        };
        let Some(previous) = session.undo.take() else {
            return HttpResponse::refusal(
                "there is nothing to undo: this session keeps one level of undo, the state before \
                 the last paint",
                &[],
            );
        };
        session.map = previous;
        // The drawn-cell account rolls back with the map. One paint's worth, not the whole
        // session's: everything painted before the undone paint is still in the map.
        session.drawn_cells -= session.undo_drawn_cells;
        session.paints -= 1;
        session.undo_drawn_cells = 0;
        HttpResponse::json(format!(
            "{{\"ok\":true,\"tiles\":{},\"notes\":[]}}",
            tiles_json(&session.map)
        ))
    }

    fn save(&mut self, fields: &BTreeMap<String, String>) -> HttpResponse {
        let Some(session) = self.session.as_ref() else {
            return HttpResponse::refusal("no map is open", &[]);
        };
        let Some(target) = fields.get("path").filter(|value| !value.is_empty()) else {
            return HttpResponse::refusal("no output path was given", &[]);
        };
        let target = PathBuf::from(target);
        match save_session(session, &target) {
            Ok(bytes) => {
                let mut notes = Vec::new();
                if session.drawn_cells > 0 {
                    notes.push(format!(
                        "{} cells in this file were drawn among equally valid tiles across {} \
                         paints. They are a legal choice, not the engine's: the engine draws at \
                         random there and that draw cannot be reproduced.",
                        session.drawn_cells, session.paints
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
        .map_err(|error| format!("could not create {}: {error}", target.display()))?;
    if let Err(error) = file.write_all(&encoded) {
        drop(file);
        let _ = fs::remove_file(target);
        return Err(format!("could not write {}: {error}", target.display()));
    }
    Ok(encoded.len())
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
        "{{\"ok\":true,\"path\":{},\"width\":{},\"height\":{},\"class\":{},\
         \"tileset\":{{\"member\":{},\"provenance\":{},\"atlas\":{},\"columns\":{},\"rows\":{},\
         \"tileWidth\":{},\"tileHeight\":{}}},\"terrains\":{terrains},\"tiles\":{},\"notes\":{}}}",
        json_string(&path.display().to_string()),
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
            b'%' if index + 2 < bytes.len() => {
                match u8::from_str_radix(&value[index + 1..index + 3], 16) {
                    Ok(byte) => {
                        out.push(byte);
                        index += 3;
                    }
                    Err(_) => {
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

/// Answer requests on `server` until it stops yielding them.
pub fn run(server: &tiny_http::Server, source: TileSetSource) {
    let mut editor = Editor::new(source);
    for mut request in server.incoming_requests() {
        let method = request.method().as_str().to_owned();
        let target = request.url().to_owned();
        let mut body = String::new();
        if let Err(error) = request.as_reader().read_to_string(&mut body) {
            eprintln!("could not read the request body: {error}");
            continue;
        }
        let response = editor.handle(&method, &target, &body);
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
    run(&server, source);
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
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            Self::with_atlas(name, FIXTURE_COLUMNS, FIXTURE_ROWS)
        }

        fn with_atlas(name: &str, columns: u32, rows: u32) -> Self {
            let dir = scratch_dir(name);
            let map = dir.join("in.scn");
            fs::write(&map, grass_map(FIXTURE_WIDTH, FIXTURE_HEIGHT)).unwrap();
            fs::write(dir.join("fixture.til"), FIXTURE_TILESET).unwrap();
            fs::write(dir.join("fixture.lbm"), fixture_atlas(columns, rows)).unwrap();
            let editor = Editor::new(TileSetSource::Loose {
                definition: dir.join("fixture.til"),
                atlas: dir.join("fixture.lbm"),
            });
            Self { dir, map, editor }
        }

        fn open(&mut self) -> String {
            let target = format!("/api/open?path={}", encode(&self.map.display().to_string()));
            let response = self.editor.handle("GET", &target, "");
            String::from_utf8(response.body).unwrap()
        }

        fn post(&mut self, path: &str, body: &str) -> String {
            let response = self.editor.handle("POST", path, body);
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

    fn refusal(json: &str) -> String {
        assert!(json.contains("\"ok\":false"), "expected a refusal: {json}");
        let start = json.find("\"refusal\":\"").expect("refusal") + "\"refusal\":\"".len();
        let end = start + json[start..].find("\",\"notes\"").unwrap();
        json[start..end].to_owned()
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
        let mut editor = Editor::new(TileSetSource::Archive(PathBuf::from("/nonexistent.mpq")));
        let page = editor.handle("GET", "/", "");
        assert_eq!(page.status, 200);
        assert!(String::from_utf8(page.body).unwrap().contains("<canvas id=\"map\">"));
        assert_eq!(editor.handle("GET", "/app.js", "").status, 200);
        assert_eq!(editor.handle("GET", "/style.css", "").status, 200);
        // Nothing else is reachable: this process reads and writes local files, so an
        // unrecognised path must not become a file read.
        let missing = editor.handle("GET", "/../../etc/passwd", "");
        assert_eq!(missing.status, 404);
        assert_eq!(editor.handle("POST", "/", "").status, 404);
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
        let response = fixture.editor.handle("GET", "/api/atlas.png", "");
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
            refusal(&fixture.post("/api/undo", "")).contains("nothing to undo"),
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
        assert!(exists.contains("could not create"), "{exists}");
        assert_eq!(fs::read(&occupied).unwrap(), b"not a map");

        // A class change: a world map saved under a combat extension would be drawn through a
        // different tileset entirely.
        let wrong_class = fixture.dir.join("out.smp");
        let class = refusal(&fixture.save(&wrong_class));
        assert!(class.contains("read through different tilesets"), "{class}");
        assert!(!wrong_class.exists(), "a refused save left a file behind");
    }

    #[test]
    fn undo_restores_the_grid_and_rolls_back_one_paint_s_worth_of_the_draw_account() {
        let mut fixture = Fixture::new("undo");
        let opened = tiles_field(&fixture.open());
        // Two paints, each with one cell the engine would have drawn at random.
        fixture.paint((4, 1, 6, 3), 2, None);
        let after_first: Vec<u32> = fixture
            .editor
            .session()
            .unwrap()
            .map
            .cells
            .iter()
            .map(MapCellTile::tile)
            .collect();
        fixture.paint((1, 1, 3, 3), 2, None);

        let undone = fixture.post("/api/undo", "");
        assert!(undone.contains("\"ok\":true"), "{undone}");
        assert_eq!(tiles_field(&undone), after_first, "undo did not restore the grid");
        assert_ne!(after_first, opened, "the fixture painted nothing");

        // One level only, by design for v1 -- and it says so rather than silently doing nothing.
        assert!(refusal(&fixture.post("/api/undo", "")).contains("one level of undo"));

        // The draw account rolls back by one paint, not to zero: the first paint's drawn cell is
        // still in the map, and a save that called this file reproducible would be lying about it.
        let json = fixture.save(&fixture.dir.join("out.scn").clone());
        assert!(
            json.contains("1 cells in this file were drawn"),
            "the undo reset the whole session's draw account: {json}"
        );
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
        thread::spawn(move || run(&server, source));

        let (status, body) = raw_request(
            address,
            &format!(
                "GET /api/open?path={} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
                encode(&map.display().to_string())
            ),
        );
        assert!(status.contains("200"), "{status}");
        assert!(body.contains("\"ok\":true"), "{body}");

        let paint = "x0=5&y0=2&x1=5&y1=2&terrain=2";
        let (status, body) = raw_request(
            address,
            &format!(
                "POST /api/paint HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\
                 Content-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n{paint}",
                paint.len()
            ),
        );
        assert!(status.contains("200"), "{status}");
        assert!(body.contains("\"tile\":12"), "the POST body never reached the handler: {body}");

        let output = dir.join("out.scn");
        let save = format!("path={}", encode(&output.display().to_string()));
        let (_, body) = raw_request(
            address,
            &format!(
                "POST /api/save HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\
                 Content-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n{save}",
                save.len()
            ),
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
            "GET /nope HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        );
        assert!(status.contains("404"), "{status}");
        let _ = fs::remove_dir_all(&dir);
    }
}
