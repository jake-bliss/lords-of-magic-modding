use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::ops::Range;

/// One of the eight neighbour columns of a `TILE=` line.
///
/// The `.til` header names the columns `n, ne, e, se, s, sw, w, nw`, and
/// [`offset`](Self::offset) is the `(dx, dy)` each one means. That mapping is **derived, not
/// assumed** -- see [`Direction::offset`] for the artifact it was derived against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Direction {
    North,
    NorthEast,
    East,
    SouthEast,
    South,
    SouthWest,
    West,
    NorthWest,
}

impl Direction {
    /// The eight directions in the order the `.til` columns appear.
    pub const ALL: [Direction; 8] = [
        Direction::North,
        Direction::NorthEast,
        Direction::East,
        Direction::SouthEast,
        Direction::South,
        Direction::SouthWest,
        Direction::West,
        Direction::NorthWest,
    ];

    /// The `(dx, dy)` step from a cell to the neighbour this column constrains.
    ///
    /// **Derived, 2026-09-17**, against `artifacts/engine-probe-captures/terrainrings-20260917`.
    /// A `.til` column could mean either "the neighbour in this geometric direction" or "the
    /// direction from that neighbour back to me" -- the two are mirror images, and a wrong choice
    /// produces maps that look plausible and are systematically flipped. The engine's own saved
    /// maps settle it: over the eight blending backgrounds times eight painted terrains times the
    /// eight ring cells, **576 of 576** ring tiles satisfy their own constraints under the
    /// geometric reading and **0 of 576** under the mirrored one, and each of the eight columns
    /// discriminates individually rather than only the system as a whole.
    ///
    /// Concretely, on `zr6.scn` the ring cell one step north of a blob holds tile 2, whose only
    /// non-plains column is `s` -- the paint is to its south, which is where the paint actually
    /// is. `y` increases southwards, as everywhere else in this writer.
    pub const fn offset(self) -> (i32, i32) {
        match self {
            Direction::North => (0, -1),
            Direction::NorthEast => (1, -1),
            Direction::East => (1, 0),
            Direction::SouthEast => (1, 1),
            Direction::South => (0, 1),
            Direction::SouthWest => (-1, 1),
            Direction::West => (-1, 0),
            Direction::NorthWest => (-1, -1),
        }
    }

    /// The direction a column name names, matched case-insensitively.
    ///
    /// The inverse of [`column_name`](Self::column_name), for a caller naming a column on a command
    /// line. It is deliberately exhaustive over [`ALL`](Self::ALL) rather than a hand-written match,
    /// so a renamed column cannot be accepted here and rejected there.
    pub fn from_column_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|direction| direction.column_name().eq_ignore_ascii_case(name))
    }

    /// The column name the `.til` header uses.
    pub const fn column_name(self) -> &'static str {
        match self {
            Direction::North => "n",
            Direction::NorthEast => "ne",
            Direction::East => "e",
            Direction::SouthEast => "se",
            Direction::South => "s",
            Direction::SouthWest => "sw",
            Direction::West => "w",
            Direction::NorthWest => "nw",
        }
    }
}

/// What one neighbour column of a `TILE=` line requires of that neighbour's terrain type.
///
/// The file's syntax is `*` for don't-care, `6` or `6|9` for an allowed set, and `~6` or `~6|9`
/// for a forbidden set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NeighbourConstraint {
    /// `*`: this neighbour is not looked at.
    Any,
    /// `6|9`: the neighbour's terrain type must be one of these.
    OneOf(BTreeSet<u32>),
    /// `~6|9`: the neighbour's terrain type must be none of these.
    NoneOf(BTreeSet<u32>),
}

impl NeighbourConstraint {
    /// Whether a neighbour of this terrain type satisfies the constraint.
    ///
    /// `None` is a neighbour off the edge of the map, and at this level it **satisfies every
    /// constraint**. That is deliberately the permissive end of the question, and on its own it is
    /// not enough: taken alone it lets the one-sided boundary tiles compete with the interior
    /// family at every map edge, and a lowest-slot tie-break then picks the *most* wrong legal
    /// option -- a whole-map water paint came out with a complete phantom coastline, row 0 claiming
    /// land to the north. [`TileSetDefinition::select_tile`] is where that is resolved, by
    /// preferring candidates that still match when an off-map neighbour is read as the cell's own
    /// terrain. See [`Neighbourhood::closed_with`].
    ///
    /// What the engine actually reads past a map edge is **Unknown**: every `terrainrings` blob is
    /// interior, so no saved artifact says.
    pub fn accepts(&self, terrain_type: Option<u32>) -> bool {
        match (self, terrain_type) {
            (Self::Any, _) | (_, None) => true,
            (Self::OneOf(set), Some(terrain)) => set.contains(&terrain),
            (Self::NoneOf(set), Some(terrain)) => !set.contains(&terrain),
        }
    }

    /// One neighbour column, parsed the way a file's own column is parsed, with no line number.
    ///
    /// This is the edit path's entry point, and it is deliberately the **same** function the file
    /// parser uses rather than a second one that accepts the same syntax: a constraint a caller
    /// writes and a constraint the file carries have to mean the same thing or an edited tileset
    /// stops matching its own rows.
    pub fn parse_column(field: &str) -> Result<Self, TileError> {
        Self::parse(field, None)
    }

    /// This constraint written the way a `.til` column writes it.
    ///
    /// **Reconstructed, not carried.** Over all 26 shipped tilesets this reproduces the original
    /// column text for 32,344 of 32,344 neighbour columns -- see
    /// [`TileSetDocument::field_rebuild_audit`] -- which is what says the set model loses nothing
    /// the file wrote. It can fail: a column written `9|6`, `6|6` or `06` parses to the same set as
    /// `6|9`, `6` and `6`, and would come back re-spelled. No shipped column is written that way.
    pub fn to_column(&self) -> String {
        match self {
            Self::Any => "*".to_owned(),
            Self::OneOf(set) => join_terrain_types(set),
            Self::NoneOf(set) => format!("~{}", join_terrain_types(set)),
        }
    }

    /// Every terrain type this constraint names, in either sense. Empty for [`Any`](Self::Any).
    pub fn terrain_types(&self) -> Vec<u32> {
        match self {
            Self::Any => Vec::new(),
            Self::OneOf(set) | Self::NoneOf(set) => set.iter().copied().collect(),
        }
    }

    fn parse(field: &str, line: Option<usize>) -> Result<Self, TileError> {
        let field = field.trim();
        if field == "*" {
            return Ok(Self::Any);
        }
        let (negated, body) = match field.strip_prefix('~') {
            Some(rest) => (true, rest),
            None => (false, field),
        };
        let mut set = BTreeSet::new();
        for alternative in body.split('|') {
            set.insert(parse_u32(alternative, line, "neighbour terrain type")?);
        }
        if set.is_empty() {
            return Err(TileError::new(format!(
                "empty neighbour constraint{}",
                at_line(line)
            )));
        }
        Ok(if negated {
            Self::NoneOf(set)
        } else {
            Self::OneOf(set)
        })
    }
}

/// How a terrain type may be crossed: column `a` of a `TERRAINTYPE=` line.
///
/// `tilesb01.til`'s header reads `a=flags (0=land,1=water,2=impassable)`. `tilesa01.til`'s reads
/// `a=flags (1=water, 0=land)` and never uses 2, which is why [`Unrecognised`](Self::Unrecognised)
/// exists rather than a parse failure: the two shipped files do not declare the same vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Passability {
    Land,
    Water,
    Impassable,
    Unrecognised(u32),
}

impl Passability {
    /// The number a `.til` writes for this passability: the inverse of [`from_value`](Self::from_value).
    ///
    /// Round-tripping through [`Unrecognised`](Self::Unrecognised) is why that variant exists rather
    /// than a parse failure -- a value neither shipped header names still writes back as itself.
    pub const fn value(self) -> u32 {
        match self {
            Self::Land => 0,
            Self::Water => 1,
            Self::Impassable => 2,
            Self::Unrecognised(value) => value,
        }
    }

    fn from_value(value: u32) -> Self {
        match value {
            0 => Self::Land,
            1 => Self::Water,
            2 => Self::Impassable,
            other => Self::Unrecognised(other),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerrainTypeDefinition {
    pub index: u32,
    pub palette_color: u32,
    pub description: String,
    /// Column `a`. `None` when the line stops before it.
    pub passability: Option<Passability>,
    /// Column `b`. The header says `b=1000` is 1.0 in the map model.
    pub min_elevation: Option<u32>,
    /// Column `c`.
    pub max_elevation: Option<u32>,
    /// Column `f`.
    pub movement_cost: Option<u32>,
    /// Columns `d`, `e`, `g` and `h`, in file order, exactly as written.
    ///
    /// **Deliberately unnamed.** `tilesb01.til` marks all four `unused`; `tilesa01.til` calls `d`
    /// food and `e` ore. The two shipped files disagree, so this writer records the text and
    /// invents no meaning for it.
    pub unnamed_fields: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TileDefinition {
    /// The atlas slot -- the `tilenum` column, and the key this tile is stored under.
    pub index: u32,
    /// The `self` column: the terrain type a cell holding this tile belongs to.
    pub terrain_type: u32,
    /// The eight neighbour columns, in [`Direction::ALL`] order.
    ///
    /// Only meaningful when [`constraints_are_complete`](Self::constraints_are_complete). A row that
    /// stopped early leaves [`NeighbourConstraint::Any`] here, which reads as "matches anything" and
    /// would make the tile selectable for every neighbourhood -- so completeness is tracked
    /// separately rather than inferred from the contents.
    pub neighbours: [NeighbourConstraint; 8],
    /// Whether the row declared all eight neighbour columns **readably**.
    ///
    /// **Every `TILE=` row in all 26 shipped tilesets has exactly 11 fields** -- 4,043 rows, checked
    /// 2026-09-17 -- so no shipped tile is incomplete and this is false only for a malformed or
    /// truncated file. Such a tile stays parsed and inspectable, and is excluded from
    /// [`candidates`](TileSetDefinition::candidates) so it can never be painted.
    pub constraints_declared: bool,
    /// The trailing `index` column, which is **not** the atlas slot.
    ///
    /// It is a pattern id shared across terrain blocks: tile 15 and tile 63 both carry 15 and both
    /// declare "all four cardinals are not my own terrain". It is also **not reliable** -- tile 2
    /// carries 6 where its pattern is plainly 2, and water's tile 50 carries 1 where tile 2's
    /// analogue is 2. Nothing in this writer decides anything from it; it is captured because
    /// discarding a column silently is how a format claim goes untested.
    pub pattern_index: Option<u32>,
}

impl TileDefinition {
    /// A tile that constrains none of its neighbours, as a `TILE=` line with no neighbour columns
    /// does.
    pub fn unconstrained(index: u32, terrain_type: u32) -> Self {
        Self {
            index,
            terrain_type,
            neighbours: std::array::from_fn(|_| NeighbourConstraint::Any),
            constraints_declared: true,
            pattern_index: None,
        }
    }

    /// Whether this tile declared all eight neighbour columns.
    ///
    /// A tile that did not cannot be selected: see [`constraints_declared`](Self::constraints_declared).
    pub fn constraints_are_complete(&self) -> bool {
        self.constraints_declared
    }

    /// The constraint on one direction.
    pub fn neighbour(&self, direction: Direction) -> &NeighbourConstraint {
        let position = Direction::ALL
            .iter()
            .position(|candidate| *candidate == direction)
            .expect("Direction::ALL contains every direction");
        &self.neighbours[position]
    }

    /// Whether this tile's eight constraints accept `neighbours`.
    ///
    /// **A tile whose row did not declare all eight columns accepts nothing.** Treating its
    /// unwritten columns as `*` would make it a wildcard selectable for any neighbourhood, which is
    /// the opposite of what a missing declaration means.
    pub fn accepts(&self, neighbours: &Neighbourhood) -> bool {
        self.constraints_declared
            && Direction::ALL
                .iter()
                .all(|direction| self.neighbour(*direction).accepts(neighbours.get(*direction)))
    }
}

/// The eight neighbouring terrain types of one cell, `None` where the neighbour is off the map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Neighbourhood([Option<u32>; 8]);

impl Neighbourhood {
    /// A neighbourhood from a lookup of `(dx, dy)` to terrain type.
    pub fn from_lookup(mut lookup: impl FnMut(i32, i32) -> Option<u32>) -> Self {
        let mut slots = [None; 8];
        for (slot, direction) in slots.iter_mut().zip(Direction::ALL) {
            let (dx, dy) = direction.offset();
            *slot = lookup(dx, dy);
        }
        Self(slots)
    }

    pub fn get(&self, direction: Direction) -> Option<u32> {
        let position = Direction::ALL
            .iter()
            .position(|candidate| *candidate == direction)
            .expect("Direction::ALL contains every direction");
        self.0[position]
    }

    pub fn set(&mut self, direction: Direction, terrain_type: Option<u32>) {
        let position = Direction::ALL
            .iter()
            .position(|candidate| *candidate == direction)
            .expect("Direction::ALL contains every direction");
        self.0[position] = terrain_type;
    }

    /// How many of the eight neighbours are off the map.
    pub fn off_map(&self) -> usize {
        self.0.iter().filter(|slot| slot.is_none()).count()
    }

    /// This neighbourhood with every off-map neighbour read as `terrain_type`.
    ///
    /// The map edge, closed rather than open. Matching against this is what stops the one-sided
    /// boundary tiles from competing with the interior family along an edge: a cell at the corner of
    /// a water map has no land anywhere near it, so the tiles that *assert* land beyond the edge
    /// should lose to the ones that do not.
    ///
    /// Because an off-map neighbour satisfies every constraint in the open reading, the candidates
    /// this produces are always a **subset** of the open ones -- so preferring them narrows a tie
    /// and never invents a tile the open reading rejected.
    pub fn closed_with(&self, terrain_type: u32) -> Self {
        Self(self.0.map(|slot| Some(slot.unwrap_or(terrain_type))))
    }
}

/// How to choose when several tiles match a neighbourhood equally.
///
/// **The engine picks at random, and that choice cannot be reproduced.** The same 3x3 painted twice
/// at the same coordinates gave centre tiles 385 and 390 with byte-identical rings. So a writer has
/// to choose something; this type makes the choice explicit rather than letting one fall out of
/// iteration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TileSelector {
    /// The lowest matching atlas slot. Deterministic, and the default.
    #[default]
    LowestSlot,
    /// A seeded pick among the matching slots, for callers that want variety without randomness.
    ///
    /// This imitates the engine's *variety*, never its actual draw.
    Seeded(u64),
}

impl TileSelector {
    fn choose(self, candidates: &[u32], x: u32, y: u32) -> u32 {
        match self {
            Self::LowestSlot => candidates[0],
            Self::Seeded(seed) => {
                // A small deterministic mix of the seed and the cell, so one seed gives one map.
                let mut state = seed
                    ^ (u64::from(x) << 32)
                    ^ u64::from(y).wrapping_mul(0x9E37_79B9_7F4A_7C15);
                state ^= state >> 33;
                state = state.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
                state ^= state >> 33;
                let index = usize::try_from(state % candidates.len() as u64)
                    .expect("a modulus of a nonzero length fits usize");
                candidates[index]
            }
        }
    }
}

/// The outcome of re-selecting one cell's tile from the tileset's constraints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TileChoice {
    /// Exactly one tile of this terrain matches. This is the reproducible case.
    Unique(u32),
    /// Several tiles match equally and the cell **already held one of them**, so it keeps it.
    ///
    /// **This is reproducible, and it is most of the ambiguous population.** Where a paint does not
    /// change what a cell's neighbourhood looks like, the engine leaves the cell alone rather than
    /// redrawing it -- in `zr0.scn` a water blob painted onto dirt leaves ring cell `(13, 5)` at
    /// tile `175` although `[31, 79, 127, 175, 223, 271]` all match it. Keeping the tile is
    /// therefore not a tie-break of convenience; it is what the engine was observed doing.
    Kept { tile: u32, candidates: Vec<u32> },
    /// Several match equally and the cell held none of them, so one had to be picked.
    ///
    /// `chosen` is this writer's [`TileSelector`] applied to `candidates`. This is the genuinely
    /// unreproducible case: a **newly painted** cell with several equally valid interiors, where the
    /// engine draws at random. It is not "any cell with more than one candidate" -- that looser
    /// reading is what produced an earlier over-claim.
    Drawn { chosen: u32, candidates: Vec<u32> },
    /// No tile of this terrain type accepts this neighbourhood.
    ///
    /// The engine writes something anyway -- on a painted `tt_impassible` rectangle it fills from
    /// slots `464..=471` although none of them accepts a non-impassable neighbour -- so this is a
    /// case the declared constraints genuinely do not cover, and a writer should refuse rather
    /// than imitate.
    NoCandidate,
}

impl TileChoice {
    /// The tile to write, or `None` when nothing matched.
    pub fn tile(&self) -> Option<u32> {
        match self {
            Self::Unique(tile) | Self::Kept { tile, .. } => Some(*tile),
            Self::Drawn { chosen, .. } => Some(*chosen),
            Self::NoCandidate => None,
        }
    }

    /// Whether several tiles matched equally, however the choice among them was made.
    pub fn is_ambiguous(&self) -> bool {
        matches!(self, Self::Kept { .. } | Self::Drawn { .. })
    }

    /// Whether this writer's answer is the engine's answer.
    ///
    /// True for [`Unique`](Self::Unique) and [`Kept`](Self::Kept); false for
    /// [`Drawn`](Self::Drawn), which is the random interior, and for
    /// [`NoCandidate`](Self::NoCandidate).
    pub fn is_reproducible(&self) -> bool {
        matches!(self, Self::Unique(_) | Self::Kept { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TileSetDefinition {
    pub atlas_member: String,
    pub columns: u32,
    pub rows: u32,
    pub tile_width: u32,
    pub tile_height: u32,
    pub terrain_types: BTreeMap<u32, TerrainTypeDefinition>,
    pub tiles: BTreeMap<u32, TileDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TileError(String);

impl TileError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for TileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for TileError {}

impl TileSetDefinition {
    /// Read a `.til`'s values.
    ///
    /// A file this cannot read is **refused**, and a refusal whose message names the wrong cause is
    /// worth little: see [`bare_cr_diagnosis`], which replaces "tile definition has no TILES
    /// dimensions" on a bare-CR file with a message naming the line endings.
    pub fn parse(source: &[u8]) -> Result<Self, TileError> {
        Self::parse_records(source).map_err(|error| bare_cr_diagnosis(source).unwrap_or(error))
    }

    fn parse_records(source: &[u8]) -> Result<Self, TileError> {
        let text = std::str::from_utf8(source)
            .map_err(|error| TileError::new(format!("tile definition is not UTF-8: {error}")))?;
        let mut atlas_member = None;
        let mut dimensions = None;
        let mut tile_size = None;
        let mut terrain_types = BTreeMap::new();
        let mut tiles = BTreeMap::new();

        for (line_index, (original_line, _)) in split_terminated_lines(text).into_iter().enumerate()
        {
            let line_number = line_index + 1;
            let line = original_line
                .split_once(';')
                .map_or(original_line, |(before, _)| before)
                .trim();
            if line.is_empty() {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim().to_ascii_uppercase();
            let value = value.trim();
            match key.as_str() {
                "LBM" => atlas_member = Some(value.to_owned()),
                "TILES" => dimensions = Some(parse_pair(value, Some(line_number), "TILES")?),
                "TILESIZE" => tile_size = Some(parse_pair(value, Some(line_number), "TILESIZE")?),
                "TERRAINTYPE" => {
                    let fields = csv_fields(value, Some(line_number))?;
                    if fields.len() < 3 {
                        return Err(TileError::new(format!(
                            "TERRAINTYPE on line {line_number} has fewer than three fields"
                        )));
                    }
                    let index = parse_u32(&fields[0], Some(line_number), "terrain type index")?;
                    let palette_color =
                        parse_u32(&fields[1], Some(line_number), "terrain palette color")?;
                    // Columns a..h follow the description. Only the four the file's own header
                    // names are given a name here; d, e, g and h are kept as text because the two
                    // shipped tilesets disagree about what d and e mean.
                    // Unreadable trailing columns are recorded as absent, not rejected, for the
                    // same reason as the tile rows: the renderer does not read them.
                    let trailing = |position: usize, name: &str| -> Option<u32> {
                        fields
                            .get(position)
                            .filter(|field| !field.is_empty())
                            .and_then(|field| parse_u32(field, Some(line_number), name).ok())
                    };
                    let definition = TerrainTypeDefinition {
                        index,
                        palette_color,
                        description: fields[2].clone(),
                        passability: trailing(3, "terrain passability flag")
                            .map(Passability::from_value),
                        min_elevation: trailing(4, "terrain minimum elevation"),
                        max_elevation: trailing(5, "terrain maximum elevation"),
                        movement_cost: trailing(8, "terrain movement cost"),
                        unnamed_fields: [6, 7, 9, 10]
                            .into_iter()
                            .filter_map(|position| fields.get(position).cloned())
                            .collect(),
                    };
                    if terrain_types.insert(index, definition).is_some() {
                        return Err(TileError::new(format!(
                            "duplicate terrain type {index} on line {line_number}"
                        )));
                    }
                }
                "TILE" => {
                    let fields = csv_fields(value, Some(line_number))?;
                    if fields.len() < 2 {
                        return Err(TileError::new(format!(
                            "TILE on line {line_number} has fewer than two fields"
                        )));
                    }
                    let index = parse_u32(&fields[0], Some(line_number), "tile index")?;
                    let terrain_type = parse_u32(&fields[1], Some(line_number), "tile terrain type")?;
                    // A row that stops before the eight neighbour columns is **recorded as
                    // incomplete**, not padded with wildcards. Padding would make the tile match
                    // every neighbourhood and so be selectable everywhere -- the opposite of what a
                    // missing declaration means.
                    //
                    // **Corrected 2026-09-17.** This code previously padded, and its comment
                    // justified the leniency by claiming `tilesa01.til` "is already a different
                    // shape from `tilesb01.til`". That is factually wrong: the two differ in row
                    // count, 609 against 617, and not in field shape. Every `TILE=` row in all 26
                    // shipped tilesets has exactly 11 fields -- 4,043 rows, counted -- and so does
                    // every one of the 402 `TERRAINTYPE=` rows. There was no shipped file the
                    // leniency was serving. Short rows are still parsed rather than rejected, so a
                    // malformed file stays inspectable through `--view-map`, but they cannot paint.
                    let mut neighbours = std::array::from_fn(|_| NeighbourConstraint::Any);
                    let mut constraints_declared = true;
                    for (slot, position) in neighbours.iter_mut().zip(2..10) {
                        // A column that is missing *or unreadable* makes the tile incomplete
                        // rather than failing the file. `--view-map` and `--export-map-preview`
                        // need only the slot and the `self` column, and hard-erroring on a
                        // constraint neither of them reads would turn a renderable modded tileset
                        // into an unopenable one -- a failure mode this parser did not have while
                        // it was discarding these columns, and must not acquire by reading them.
                        match fields
                            .get(position)
                            .filter(|field| !field.is_empty())
                            .map(|field| NeighbourConstraint::parse(field, Some(line_number)))
                        {
                            Some(Ok(constraint)) => *slot = constraint,
                            Some(Err(_)) | None => constraints_declared = false,
                        }
                    }
                    // Likewise the trailing pattern column: nothing decides anything from it, so an
                    // unreadable one is recorded as absent rather than rejected.
                    let pattern_index = fields
                        .get(10)
                        .filter(|field| !field.is_empty())
                        .and_then(|field| parse_u32(field, Some(line_number), "tile pattern index").ok());
                    let definition = TileDefinition {
                        index,
                        terrain_type,
                        neighbours,
                        constraints_declared,
                        pattern_index,
                    };
                    if tiles.insert(index, definition).is_some() {
                        return Err(TileError::new(format!(
                            "duplicate tile {index} on line {line_number}"
                        )));
                    }
                }
                _ => {}
            }
        }

        let atlas_member =
            atlas_member.ok_or_else(|| TileError::new("tile definition has no LBM"))?;
        let (columns, rows) =
            dimensions.ok_or_else(|| TileError::new("tile definition has no TILES dimensions"))?;
        let (tile_width, tile_height) =
            tile_size.ok_or_else(|| TileError::new("tile definition has no TILESIZE"))?;
        if columns == 0 || rows == 0 || tile_width == 0 || tile_height == 0 {
            return Err(TileError::new("tile and atlas dimensions must be nonzero"));
        }
        let capacity = columns
            .checked_mul(rows)
            .ok_or_else(|| TileError::new("tile atlas capacity overflow"))?;
        if let Some(index) = tiles.keys().find(|index| **index >= capacity) {
            return Err(TileError::new(format!(
                "tile {index} exceeds declared atlas capacity {capacity}"
            )));
        }

        Ok(Self {
            atlas_member,
            columns,
            rows,
            tile_width,
            tile_height,
            terrain_types,
            tiles,
        })
    }

    pub fn atlas_capacity(&self) -> u32 {
        self.columns * self.rows
    }

    /// The terrain type a cell holding `tile_index` belongs to.
    ///
    /// **This is what makes an arbitrary map readable.** The eleven-entry representative-tile table
    /// covers eleven slots; a `.til` declares the `self` column for essentially the whole atlas --
    /// 617 of `tilesb01.til`'s 624 -- so a shipped map's existing terrain can be read off its tiles
    /// instead of being refused as unrecognised.
    pub fn terrain_type_of_tile(&self, tile_index: u32) -> Option<u32> {
        self.tiles.get(&tile_index).map(|tile| tile.terrain_type)
    }

    /// Every tile of `terrain_type` whose eight constraints accept `neighbours`, lowest slot first.
    ///
    /// A tile whose row did not declare all eight columns is **excluded**: see
    /// [`TileDefinition::accepts`]. It stays parsed and inspectable and can never be painted.
    pub fn candidates(&self, terrain_type: u32, neighbours: &Neighbourhood) -> Vec<u32> {
        self.tiles
            .values()
            .filter(|tile| tile.terrain_type == terrain_type && tile.accepts(neighbours))
            .map(|tile| tile.index)
            .collect()
    }

    /// Re-select a cell's tile: the tile a cell of `terrain_type` should hold given `neighbours`.
    ///
    /// **Derived, 2026-09-17, re-measured after the keep-current rule landed.** Applied to the
    /// `terrainrings` captures over all 121 background-by-painted rows, this reproduces the engine
    /// on **2,084 of 2,084** cells it claims to decide, with zero mismatches. Those 2,084 are two
    /// populations, and the distinction matters:
    ///
    /// | outcome | cells | agree with the engine |
    /// | --- | --- | --- |
    /// | [`Unique`](TileChoice::Unique) -- one candidate | 1,888 | **1,888** |
    /// | [`Kept`](TileChoice::Kept) -- several candidates, the cell already held one | 196 | **196** |
    /// | [`Drawn`](TileChoice::Drawn) -- several candidates, the cell held none | 253 | 44, by coincidence |
    /// | [`NoCandidate`](TileChoice::NoCandidate) -- road and impassable | 512 | 0; the engine writes one anyway |
    ///
    /// **An earlier version of this comment claimed all 2,084 had "a single candidate". That was
    /// wrong: 196 of them have between two and sixteen.** They are reproducible because the engine
    /// leaves such a cell alone, not because the tileset narrows it to one -- in `zr0.scn` a water
    /// blob painted onto dirt leaves ring cell `(13, 5)` at tile `175` although `[31, 79, 127, 175,
    /// 223, 271]` all match. Until the keep-current rule below existed, the figure was measured with
    /// a rule the code did not have.
    ///
    /// The 253 `Drawn` cells are **not** convertible by keeping: by construction they are exactly
    /// the cells holding no valid candidate, so there is nothing to keep. They are the genuine
    /// random draw -- a **newly painted** cell with several equally valid interiors, where the same
    /// paint run twice gave centre tiles 385 and 390. `Drawn` is that case and only that case, and
    /// the looser reading of "ambiguous" is what produced the over-claim above.
    ///
    /// Three things decide a cell, in order:
    ///
    /// 1. **The map edge is read closed.** An off-map neighbour satisfies every constraint on its
    ///    own, which lets the one-sided boundary tiles compete with the interior family along every
    ///    edge; a lowest-slot tie-break then picks the *most* wrong legal option. A whole-map water
    ///    paint came out with a complete phantom coastline -- row 0 tile 49, which asserts land to
    ///    the north. So candidates are taken from [`Neighbourhood::closed_with`] first, and the open
    ///    reading is used only if closing leaves nothing. Closed candidates are always a subset, so
    ///    this narrows a tie and never invents a tile.
    /// 2. **An existing valid tile is kept.** See [`TileChoice::Kept`].
    /// 3. **Otherwise the [`TileSelector`] picks**, and the result is [`TileChoice::Drawn`].
    pub fn select_tile(
        &self,
        terrain_type: u32,
        neighbours: &Neighbourhood,
        current_tile: Option<u32>,
        selector: TileSelector,
        at: (u32, u32),
    ) -> TileChoice {
        let mut candidates = self.candidates(terrain_type, &neighbours.closed_with(terrain_type));
        if candidates.is_empty() {
            candidates = self.candidates(terrain_type, neighbours);
        }
        match candidates.len() {
            0 => TileChoice::NoCandidate,
            1 => TileChoice::Unique(candidates[0]),
            _ => match current_tile.filter(|tile| candidates.contains(tile)) {
                Some(tile) => TileChoice::Kept { tile, candidates },
                None => TileChoice::Drawn {
                    chosen: selector.choose(&candidates, at.0, at.1),
                    candidates,
                },
            },
        }
    }

}

/// Which class of map a file is, for the purpose of choosing a tileset.
///
/// The engine keeps **two** tilesets live at once and picks between them by what kind of map is
/// being drawn, not by anything stored in the map file. See [`engine_tileset_member`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MapClass {
    /// The overland map: `.scn` scenarios, `.lgd` legends, and the loose `.map`.
    World,
    /// A combat map: the 337 `.smp` special maps.
    Combat,
}

impl MapClass {
    /// The class a map file's extension puts it in, or `None` for an extension not in the corpus.
    ///
    /// Matched **case-insensitively**, because the installed `map/` directory is split: 172 files
    /// end `.smp` and 165 end `.SMP`. A case-sensitive match would silently classify 165 combat
    /// maps as unknown, which is precisely the kind of half-working that reads as "no rule found".
    pub fn from_extension(extension: &str) -> Option<Self> {
        if extension.eq_ignore_ascii_case("smp") {
            Some(Self::Combat)
        } else if ["scn", "lgd", "map"]
            .iter()
            .any(|known| extension.eq_ignore_ascii_case(known))
        {
            Some(Self::World)
        } else {
            None
        }
    }

    /// The class of the map at `path`, from its extension.
    pub fn from_path(path: &std::path::Path) -> Option<Self> {
        Self::from_extension(path.extension()?.to_str()?)
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::World => "world map",
            Self::Combat => "combat map",
        }
    }
}

/// The `.til` member the engine reads a **world** map through.
///
/// **Observed in a local binary, 2026-09-17.** `START.GS` line 74 sets `maptileset` to
/// `til/tilesb01.til`, and every other `maptileset` call in `gs.mpq` either restores that same
/// value on the way out to the menu or passes whatever the terrain editor's `tileselector`
/// currently holds. So a `.scn`, `.lgd` or `.map` is read through `tilesb01.til`.
///
/// **There is deliberately no combat equivalent of this constant.** See
/// [`COMBAT_TILESET_BINDINGS`]: a combat map's tileset is a property of the *encounter* that loads
/// it, declared per encounter in the gamescript, and it cannot be derived from the map at all.
pub const WORLD_TILESET_MEMBER: &str = "tilesb01.til";

/// The `.til` the engine falls back to for combat it generates rather than loads.
///
/// `START.GS` line 75 is `"til/tilesa01.til"combattileset`, and `combattileset` appears **exactly
/// once in all 1,700 `gs.mpq` members** -- verified by extracting every one of them. That much is
/// measured and is not in doubt.
///
/// **What it is not: the tileset of the 337 shipped `.smp` files.** A corpus comment in
/// `wilderness_land.gs` and `wilderness_sea.gs` states the rule outright -- *"WHEN 'mapfile' AND
/// 'tileset' ARE UNDEFINED, YOU GET AN OUTSIDE COMBAT ENCOUNTER"* -- so `combattileset` is the
/// default for an encounter that names **no** map, which is the engine generating open-field
/// combat. An encounter that names a `.smp` names a tileset beside it. This constant is recorded
/// because it is real; it must not be used to resolve a `.smp`.
pub const GENERATED_COMBAT_TILESET_MEMBER: &str = "tilesa01.til";

/// Every `(combat map, tileset)` binding the shipped gamescript declares.
///
/// **Observed in a local binary, 2026-09-17, and this is the corrected answer to "which tileset
/// does a `.smp` use".** The engine does not derive it and neither can this code: each *encounter*
/// script defines `mapfile` and `tileset` as adjacent keys of its own dictionary, so the tileset
/// belongs to the encounter, not to the map file. Two encounters may load the same `.smp` through
/// different tilesets, and 26 of these entries do exactly that.
///
/// ```text
/// /mapfile"map/aicave.smp"def   /tileset"til/aibldg01.til"def
/// /mapfile"map/fienc1.smp"def   /tileset"til/cavelava.til"def
/// ```
///
/// Extracted from all 1,700 `gs.mpq` members (1,699 extractable, one being the archive's own
/// `(listfile)`), by taking every `/mapfile` definition and the `/tileset` definition nearest it
/// in the same member. A definition may be a string literal or a **procedure**, and procedure
/// bodies are included: `aimult.gs` writes `/tileset{dungeon_id getdungeonstrength 3 le{...}...}`
/// whose three branches all yield `aibldg01.til`, and `genchaos.gs` writes a `terrainsprites`-keyed
/// selector that yields `chbldg01.til` or `cavelava.til`. Both forms are folded in here, which is
/// why a map may carry several candidates.
///
/// Values are lowercase basenames and lookups are **case-insensitive**, because the scripts are
/// not consistent -- `/tileset"til/LIBLDG01.til"` occurs in uppercase, and the installed `map/`
/// directory is split 172 `.smp` to 165 `.SMP`.
///
/// **Coverage is partial and is not padded.** 169 of the 337 installed `.smp` files appear here;
/// 168 do not, and for those this code answers *unresolved* rather than guessing. Best-fit scoring
/// cannot rescue them: 110 of the 168 have **sixteen** tilesets tied within one percentage point of
/// the best, because the shipped tilesets collapse to only 16 distinct rule sets. See
/// `docs/map-format.md`.
///
/// Three entries name maps the corpus does not install (`dwl.smp`, `dwlcry.smp`,
/// `tutorialmult.smp`) and one names a tileset `pic.mpq` does not ship (`cavetile.til`). They are
/// kept because the table records what the scripts say, not what resolves.
#[rustfmt::skip]
pub static COMBAT_TILESET_BINDINGS: &[(&str, &[&str])] = &[
    ("91gauntlet.smp", &["ruins01.til"]),
    ("aibldg01.smp", &["aibldg01.til"]),
    ("aicave.smp", &["aibldg01.til"]),
    ("aicavmule.smp", &["aibldg01.til"]),
    ("aicavmulh.smp", &["aibldg01.til"]),
    ("aicavmulm.smp", &["aibldg01.til"]),
    ("aidung.smp", &["aibldg01.til"]),
    ("aigtem1.smp", &["aibldg01.til"]),
    ("aimina.smp", &["aibldg01.til"]),
    ("aiminc.smp", &["cavecrys.til"]),
    ("aiming.smp", &["aibldg01.til"]),
    ("aistat.smp", &["aibldg01.til"]),
    ("aitowe.smp", &["aibldg01.til"]),
    ("aivilg1.smp", &["aibldg01.til"]),
    ("animalcave.smp", &["cavewatr.til"]),
    ("barrow3.smp", &["debldg01.til"]),
    ("barrowl1.smp", &["debldg01.til"]),
    ("barrowl2.smp", &["debldg01.til"]),
    ("bridge.smp", &["jeff01.til"]),
    ("castle.smp", &["libldg01.til", "ruins01.til"]),
    ("cavea1.smp", &["cavewatr.til"]),
    ("caveb1.smp", &["cavewatr.til"]),
    ("caveb2.smp", &["cavewatr.til"]),
    ("cavlav03.smp", &["cavelava.til"]),
    ("chapelin.smp", &["jeff01.til", "libldg01.til"]),
    ("chapelout.smp", &["jeff01.til"]),
    ("chcave.smp", &["cavecrys.til", "cavelava.til", "cavewatr.til", "chbldg01.til", "ruins01.til"]),
    ("chcavmule.smp", &["cavewatr.til"]),
    ("chcavmulh.smp", &["cavewatr.til"]),
    ("chcavmulm.smp", &["cavewatr.til"]),
    ("chdung.smp", &["chbldg01.til", "ruins01.til"]),
    ("chgtem1.smp", &["chbldg01.til"]),
    ("chmina.smp", &["chbldg01.til"]),
    ("chminc.smp", &["cavecrys.til"]),
    ("chming.smp", &["cavelava.til"]),
    ("chstat.smp", &["chbldg01.til", "ruins01.til"]),
    ("chtgil3.smp", &["ruins01.til"]),
    ("chtowe.smp", &["cavelava.til", "chbldg01.til", "ruins01.til"]),
    ("debrks0.smp", &["cavewatr.til"]),
    ("debrks1.smp", &["ruins01.til"]),
    ("debrks2.smp", &["ruins01.til"]),
    ("debrks3.smp", &["ruins01.til"]),
    ("decave.smp", &["cavelava.til"]),
    ("decavmule.smp", &["cavewatr.til"]),
    ("decavmulh.smp", &["cavewatr.til"]),
    ("decavmulm.smp", &["cavewatr.til"]),
    ("dedung.smp", &["cavelava.til", "debldg01.til"]),
    ("deepcave.smp", &["cavewatr.til"]),
    ("degtem1.smp", &["debldg01.til"]),
    ("demina.smp", &["debldg01.til"]),
    ("deminc.smp", &["cavecrys.til"]),
    ("deming.smp", &["cavelava.til"]),
    ("destat.smp", &["debldg01.til"]),
    ("detgil1.smp", &["ruins01.til"]),
    ("detgil3.smp", &["ruins01.til"]),
    ("detowe.smp", &["debldg01.til", "ruins01.til"]),
    ("devilg0.smp", &["cavelava.til"]),
    ("dragonduel.smp", &["jeff01.til"]),
    ("dwl.smp", &["cavetile.til"]),
    ("dwlcry.smp", &["cavecrys.til"]),
    ("eacave.smp", &["libldg01.til", "ruins01.til"]),
    ("eacavmule.smp", &["libldg01.til"]),
    ("eacavmulh.smp", &["libldg01.til"]),
    ("eacavmulm.smp", &["libldg01.til"]),
    ("eadung.smp", &["cavelava.til", "cavewatr.til", "chbldg01.til", "libldg01.til"]),
    ("eagtem1.smp", &["eabldg01.til"]),
    ("eamina.smp", &["libldg01.til"]),
    ("eaminc.smp", &["cavecrys.til"]),
    ("eaming.smp", &["cavelava.til"]),
    ("eamul2.smp", &["chbldg01.til"]),
    ("eastat.smp", &["eabldg01.til"]),
    ("eatgil3.smp", &["eabldg01.til"]),
    ("eatowe.smp", &["libldg01.til", "ruins01.til"]),
    ("elvenfort.smp", &["jeff01.til"]),
    ("ficave.smp", &["cavelava.til", "cavewatr.til", "fibldg01.til"]),
    ("ficavmule.smp", &["cavewatr.til"]),
    ("ficavmulh.smp", &["cavewatr.til"]),
    ("ficavmulm.smp", &["cavewatr.til"]),
    ("fidung.smp", &["fibldg01.til"]),
    ("fienc1.smp", &["cavelava.til"]),
    ("fienc2.smp", &["cavelava.til"]),
    ("fienc4.smp", &["cavecrys.til"]),
    ("fienc4a.smp", &["cavewatr.til"]),
    ("fienc4b.smp", &["cavewatr.til"]),
    ("fienc4c.smp", &["cavecrys.til"]),
    ("fienc5.smp", &["libldg01.til"]),
    ("fienc6.smp", &["ruins01.til"]),
    ("fienc8.smp", &["cavecrys.til"]),
    ("figtem1.smp", &["fibldg01.til"]),
    ("fimina.smp", &["fibldg01.til"]),
    ("fiminc.smp", &["cavecrys.til"]),
    ("fiming.smp", &["cavelava.til"]),
    ("fistat.smp", &["fibldg01.til"]),
    ("fitowe.smp", &["cavelava.til", "chbldg01.til", "fibldg01.til", "ruins01.til"]),
    ("fivilg0.smp", &["cavelava.til"]),
    ("gatehouse.smp", &["jeff01.til"]),
    ("gauntlet1.smp", &["jeff01.til"]),
    ("gauntlet2.smp", &["jeff01.til"]),
    ("greathall.smp", &["chbldg01.til"]),
    ("guindoel1.smp", &["ruins01.til"]),
    ("hamlet.smp", &["jeff01.til"]),
    ("hamlet2.smp", &["jeff01.til"]),
    ("hamlet3.smp", &["jeff01.til"]),
    ("hamlet3a.smp", &["jeff01.til"]),
    ("hermodl1.smp", &["debldg01.til"]),
    ("hermodl2.smp", &["debldg01.til"]),
    ("hienc7.smp", &["chbldg01.til"]),
    ("home.smp", &["libldg01.til"]),
    ("lances.smp", &["ruins01.til"]),
    ("laroche1.smp", &["jeff01.til"]),
    ("librks0.smp", &["libldg01.til"]),
    ("licave.smp", &["libldg01.til", "wabldg01.til"]),
    ("licavmule.smp", &["jeff01.til"]),
    ("licavmulh.smp", &["jeff01.til"]),
    ("licavmulm.smp", &["jeff01.til"]),
    ("licha.smp", &["ruins01.til"]),
    ("lichb.smp", &["ruins01.til"]),
    ("lidung.smp", &["libldg01.til"]),
    ("ligtem1.smp", &["libldg01.til"]),
    ("limina.smp", &["libldg01.til"]),
    ("liminc.smp", &["cavecrys.til"]),
    ("liming.smp", &["cavewatr.til"]),
    ("limul2.smp", &["jeff01.til"]),
    ("listat.smp", &["libldg01.til"]),
    ("litowe.smp", &["libldg01.til"]),
    ("livilg0.smp", &["libldg01.til"]),
    ("livilg1.smp", &["libldg01.til"]),
    ("marsh.smp", &["tilesa01.til"]),
    ("mercguild.smp", &["jeff01.til"]),
    ("niceinside.smp", &["libldg01.til"]),
    ("orbrks2.smp", &["libldg01.til"]),
    ("orbrks3.smp", &["libldg01.til", "orbldg01.til"]),
    ("orcave.smp", &["orbldg01.til"]),
    ("orcavmule.smp", &["jeff01.til"]),
    ("orcavmulh.smp", &["jeff01.til"]),
    ("orcavmulm.smp", &["jeff01.til"]),
    ("orcbarracks.smp", &["chbldg01.til"]),
    ("orccity3.smp", &["jeff01.til"]),
    ("ordung.smp", &["orbldg01.til", "ruins01.til"]),
    ("orgtem1.smp", &["libldg01.til", "orbldg01.til"]),
    ("ormina.smp", &["orbldg01.til"]),
    ("orminc.smp", &["cavecrys.til"]),
    ("orming.smp", &["cavewatr.til"]),
    ("orstat.smp", &["orbldg01.til"]),
    ("ortowe.smp", &["libldg01.til", "orbldg01.til"]),
    ("pathwoods.smp", &["tilesa01.til"]),
    ("recwiztow.smp", &["chbldg01.til", "debldg01.til", "orbldg01.til", "ruins01.til"]),
    ("redcity1.smp", &["libldg01.til"]),
    ("ruinedfort.smp", &["jeff01.til"]),
    ("ruinedhall2.smp", &["ruins01.til"]),
    ("ship.smp", &["jeff01.til"]),
    ("swordnstn.smp", &["libldg01.til"]),
    ("tutorialmult.smp", &["jeff01.til"]),
    ("vortigrn.smp", &["libldg01.til", "ruins01.til"]),
    ("vortreal.smp", &["libldg01.til"]),
    ("wacave.smp", &["cavewatr.til", "ruins01.til"]),
    ("wacavmule.smp", &["jeff01.til"]),
    ("wacavmulh.smp", &["jeff01.til"]),
    ("wacavmulm.smp", &["jeff01.til"]),
    ("wadung.smp", &["wabldg01.til"]),
    ("wagtem1.smp", &["wabldg01.til"]),
    ("wamina.smp", &["wabldg01.til"]),
    ("waminc.smp", &["cavecrys.til"]),
    ("waming.smp", &["cavewatr.til"]),
    ("wamul2.smp", &["wabldg01.til"]),
    ("wastat.smp", &["wabldg01.til"]),
    ("watowe.smp", &["wabldg01.til"]),
    ("wavilg1.smp", &["debldg01.til", "wabldg01.til"]),
    ("web1.smp", &["debldg01.til"]),
    ("web2.smp", &["libldg01.til"]),
    ("web3.smp", &["debldg01.til"]),
    ("wodensford.smp", &["tilesa01.til"]),
];

/// Extra tilesets the **plural selector** form puts in reach for a map, beyond its declared one.
///
/// **Observed in a local binary, 2026-09-17. This form is live code and an earlier version of this
/// project wrongly recorded it as absent** -- the search had looked for `/tilesets[`, but it ships
/// as a procedure plus a *separately named* array:
///
/// ```text
/// /tilesets{ ... tiles 0 get ... currentterrainsprite getterrainspritelocation
///            8 mod 2 eq{pop tiles 2 get}if ... }/dummy currentdict replace
/// /tiles["til/cavewatr.til" "til/cavecrys.til" "til/cavelava.til" "til/aibldg01.til"]replace bind def
/// ```
///
/// So **one** encounter chooses among up to four tilesets at runtime from a sprite's map location,
/// which is a second and independent reason a combat map has no single tileset. 41 members define
/// `/tilesets`, 40 define `/tiles[` and 40 define `/maps[`.
///
/// **These are read coarsely: every tileset in a member's `/tiles[...]` is a candidate for every
/// map in its `/maps[...]`, minus whatever is already declared.** The arrays cannot be zipped
/// positionally with confidence -- the `/mapfiles` and `/tilesets` procedures branch on *different*
/// predicates (`4 mod 0`, `4 mod 2`, `8 mod 7` against `8 mod 2`, `8 mod 4`, `8 mod 6`, `8 mod 7`
/// in `waming.gs`) with different branches commented out in each, only 31 of 40 members have
/// equal-length arrays, and 5 of 40 disagree with their own literal pair at index 0. Evaluating the
/// predicates would need `getterrainspritelocation`, a runtime value. An over-wide set that says so
/// is honest; a narrow wrong one is not.
///
/// **Why this is kept out of [`COMBAT_TILESET_BINDINGS`] rather than merged into it.** Merging
/// costs precision and buys no coverage: all 43 maps named in a `/maps[...]` array are **already**
/// declared by a literal pair, so the coarse reading resolves **zero** additional maps, while
/// widening 42 of the 43 across more than one tileset *rule class*. It would stand `ruins01.til`
/// (81.3% satisfaction on `demina.smp`) beside the declared `debldg01.til` (98.3%) as an equal.
/// Constraint scoring cannot adjudicate the three readings -- mean best satisfaction is 95.33%
/// declared, 94.58% zipped, 95.57% crossed -- so the separation is a judgement, recorded as one.
///
/// It is still consulted by [`tileset_mismatch`], because a tileset the engine may genuinely reach
/// at runtime must not be *refused*. Reporting is precise; refusing is permissive.
#[rustfmt::skip]
pub static COMBAT_TILESET_ARRAY_CANDIDATES: &[(&str, &[&str])] = &[
    ("aicave.smp", &["cavecrys.til", "cavelava.til", "cavewatr.til"]),
    ("aimina.smp", &["cavecrys.til", "chbldg01.til", "debldg01.til", "fibldg01.til", "libldg01.til", "orbldg01.til", "ruins01.til", "wabldg01.til"]),
    ("aiminc.smp", &["cavelava.til", "cavewatr.til"]),
    ("aiming.smp", &["cavecrys.til", "cavelava.til", "cavewatr.til"]),
    ("aistat.smp", &["chbldg01.til", "debldg01.til", "fibldg01.til", "libldg01.til", "orbldg01.til", "ruins01.til", "wabldg01.til"]),
    ("chcave.smp", &["aibldg01.til", "fibldg01.til", "libldg01.til", "orbldg01.til"]),
    ("chmina.smp", &["aibldg01.til", "debldg01.til", "fibldg01.til", "orbldg01.til", "ruins01.til"]),
    ("chminc.smp", &["cavelava.til", "cavewatr.til"]),
    ("chming.smp", &["aibldg01.til", "cavecrys.til", "cavewatr.til"]),
    ("chstat.smp", &["aibldg01.til"]),
    ("decave.smp", &["aibldg01.til", "cavecrys.til", "cavewatr.til", "chbldg01.til", "debldg01.til", "ruins01.til", "wabldg01.til"]),
    ("demina.smp", &["aibldg01.til", "chbldg01.til", "ruins01.til"]),
    ("deminc.smp", &["cavelava.til", "cavewatr.til"]),
    ("deming.smp", &["aibldg01.til", "cavecrys.til", "cavewatr.til"]),
    ("destat.smp", &["aibldg01.til", "ruins01.til"]),
    ("devilg0.smp", &["cavewatr.til", "debldg01.til", "ruins01.til"]),
    ("eacave.smp", &["cavelava.til", "cavewatr.til", "debldg01.til", "orbldg01.til", "wabldg01.til"]),
    ("eamina.smp", &["aibldg01.til", "debldg01.til", "ruins01.til", "wabldg01.til"]),
    ("eaminc.smp", &["cavelava.til", "cavewatr.til"]),
    ("eaming.smp", &["aibldg01.til", "cavecrys.til", "cavewatr.til"]),
    ("ficave.smp", &["aibldg01.til", "cavecrys.til", "debldg01.til", "libldg01.til", "orbldg01.til", "ruins01.til"]),
    ("fimina.smp", &["aibldg01.til", "ruins01.til"]),
    ("fiminc.smp", &["cavelava.til", "cavewatr.til"]),
    ("fiming.smp", &["aibldg01.til", "cavecrys.til", "cavewatr.til"]),
    ("fistat.smp", &["aibldg01.til", "ruins01.til"]),
    ("fitowe.smp", &["cavewatr.til"]),
    ("fivilg0.smp", &["cavewatr.til", "fibldg01.til", "ruins01.til"]),
    ("licave.smp", &["cavelava.til", "cavewatr.til", "ruins01.til"]),
    ("limina.smp", &["aibldg01.til", "cavecrys.til", "orbldg01.til", "ruins01.til", "wabldg01.til"]),
    ("liminc.smp", &["cavelava.til", "cavewatr.til"]),
    ("liming.smp", &["aibldg01.til", "cavecrys.til", "cavelava.til"]),
    ("listat.smp", &["aibldg01.til", "ruins01.til"]),
    ("orcave.smp", &["cavelava.til", "cavewatr.til", "chbldg01.til", "fibldg01.til", "libldg01.til", "ruins01.til"]),
    ("ormina.smp", &["aibldg01.til", "libldg01.til", "ruins01.til"]),
    ("orminc.smp", &["cavelava.til", "cavewatr.til"]),
    ("orming.smp", &["aibldg01.til", "cavecrys.til", "cavelava.til"]),
    ("orstat.smp", &["aibldg01.til", "libldg01.til", "ruins01.til", "wabldg01.til"]),
    ("wacave.smp", &["cavelava.til", "libldg01.til", "wabldg01.til"]),
    ("wamina.smp", &["aibldg01.til", "cavecrys.til", "chbldg01.til", "fibldg01.til", "libldg01.til", "ruins01.til"]),
    ("waminc.smp", &["cavelava.til", "cavewatr.til"]),
    ("waming.smp", &["aibldg01.til", "cavecrys.til", "cavelava.til"]),
    ("wastat.smp", &["aibldg01.til", "ruins01.til"]),
];

/// What the gamescript says a map should be read through.
///
/// Every resolving variant carries a **slice**, because ambiguity is a real outcome here rather
/// than an error case: a combat map may legitimately have several tilesets, so "the answer" is a
/// set and the single-answer case is just a set of one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TileSetResolution {
    /// A world map: one answer, from `maptileset`.
    World(&'static [&'static str]),
    /// A combat map the scripts bind to exactly one tileset.
    Combat(&'static [&'static str]),
    /// A combat map different encounters load through different tilesets.
    ///
    /// **This is a finding, not a gap.** `chcave.smp` is drawn through `cavelava.til`,
    /// `chbldg01.til`, `cavewatr.til`, `ruins01.til` and `cavecrys.til` by five different
    /// encounters. There is no single right answer, and a writer must be told so rather than
    /// handed the first one.
    CombatAmbiguous(&'static [&'static str]),
    /// A combat map no gamescript binding covers. **Unresolved, and not defaulted.**
    CombatUnresolved,
}

/// The one-element candidate list every world map resolves to.
static WORLD_TILESETS: &[&str] = &[WORLD_TILESET_MEMBER];

impl TileSetResolution {
    /// The single tileset this resolves to, or `None` when it does not resolve to exactly one.
    pub const fn unique(&self) -> Option<&'static str> {
        match self {
            Self::World(members) | Self::Combat(members) => Some(members[0]),
            Self::CombatAmbiguous(_) | Self::CombatUnresolved => None,
        }
    }

    /// Every tileset this map may legitimately be read through, empty when unresolved.
    pub const fn candidates(&self) -> &'static [&'static str] {
        match self {
            Self::World(members) | Self::Combat(members) | Self::CombatAmbiguous(members) => {
                members
            }
            Self::CombatUnresolved => &[],
        }
    }

    /// Whether `member` is a tileset this map may be read through.
    pub fn accepts(&self, member: &str) -> bool {
        self.candidates()
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(member))
    }

    /// A one-line description for the CLI, naming every candidate.
    pub fn describe(&self) -> String {
        match self {
            Self::World(members) | Self::Combat(members) => members[0].to_owned(),
            Self::CombatAmbiguous(members) => {
                format!("ambiguous: {}", members.join(" or "))
            }
            Self::CombatUnresolved => "unresolved".to_owned(),
        }
    }
}

/// The tilesets the gamescript binds `map_file_name` to, case-insensitively.
///
/// `None` when no binding exists. The table is sorted by map name, so this is a binary search.
pub fn combat_tileset_bindings(map_file_name: &str) -> Option<&'static [&'static str]> {
    look_up(COMBAT_TILESET_BINDINGS, map_file_name)
}

/// The extra tilesets the plural selector form puts in reach for `map_file_name`.
///
/// Empty for most maps. See [`COMBAT_TILESET_ARRAY_CANDIDATES`] for why these are separate from the
/// declared bindings and why they are nonetheless not refused.
pub fn combat_tileset_array_candidates(map_file_name: &str) -> &'static [&'static str] {
    look_up(COMBAT_TILESET_ARRAY_CANDIDATES, map_file_name).unwrap_or(&[])
}

/// Every tileset a paint may legitimately use for this map: declared, plus runtime-reachable.
///
/// **Reporting is precise and refusing is permissive**, and this is the permissive side. A tileset
/// the engine may actually reach must not be refused, because refusing the engine's own answer is
/// the bug that shipped on this branch once already.
pub fn paintable_tilesets(map_path: &std::path::Path) -> Vec<&'static str> {
    let Some(resolution) = resolve_tileset(map_path) else {
        return Vec::new();
    };
    let mut all: Vec<&'static str> = resolution.candidates().to_vec();
    if let Some(name) = map_path.file_name().and_then(|name| name.to_str()) {
        for extra in combat_tileset_array_candidates(name) {
            if !all.contains(extra) {
                all.push(extra);
            }
        }
    }
    all.sort_unstable();
    all
}

/// Binary search one of the two sorted, lowercase-keyed binding tables.
fn look_up(
    table: &'static [(&'static str, &'static [&'static str])],
    map_file_name: &str,
) -> Option<&'static [&'static str]> {
    let needle = map_file_name.to_ascii_lowercase();
    // The tables are lowercase and sorted by it, so compare on a lowercase view of the needle
    // rather than case-insensitively against an arbitrary-case key.
    table
        .binary_search_by(|(name, _)| name.cmp(&needle.as_str()))
        .ok()
        .map(|index| table[index].1)
}

/// What the engine reads the map at `path` through.
///
/// `None` only when the extension is not one the corpus classifies. A `.smp` always resolves to
/// *something*, but that something may be [`TileSetResolution::CombatUnresolved`].
pub fn resolve_tileset(path: &std::path::Path) -> Option<TileSetResolution> {
    let class = MapClass::from_path(path)?;
    if class == MapClass::World {
        return Some(TileSetResolution::World(WORLD_TILESETS));
    }
    let name = path.file_name()?.to_str()?;
    match combat_tileset_bindings(name) {
        Some(bound) if bound.len() == 1 => Some(TileSetResolution::Combat(bound)),
        Some(several) => Some(TileSetResolution::CombatAmbiguous(several)),
        None => Some(TileSetResolution::CombatUnresolved),
    }
}

/// The 26 `.til` members shipped in GS5R3 `pic.mpq`, lowercase, sorted.
///
/// Only the **names** are recorded; no tileset is committed. This exists so a supplied tileset can
/// be told apart from a modded one: a caller who passes a shipped tileset the scripts demonstrably
/// do not use for that map has made a mistake worth refusing, while a caller who passes
/// `mymod.til` has not, and must keep working.
pub const SHIPPED_TILESET_MEMBERS: [&str; 26] = [
    "aibldg01.til",
    "cavecry2.til",
    "cavecrys.til",
    "cavelava.til",
    "cavewatr.til",
    "chbldg01.til",
    "chbldg02.til",
    "chbldg0x.til",
    "debldg01.til",
    "debldg02.til",
    "eabldg01.til",
    "fibldg01.til",
    "fibldg02.til",
    "fibldg0x.til",
    "jeff01.til",
    "libldg01.til",
    "libldg0x.til",
    "orbldg01.til",
    "orbldg02.til",
    "orbldg0x.til",
    "ruins01.til",
    "ruins0x.til",
    "tilesa01.til",
    "tilesb01.til",
    "wabldg01.til",
    "wabldg02.til",
];

/// Whether `file_name` names one of the shipped tilesets, case-insensitively.
pub fn is_shipped_tileset(file_name: &str) -> bool {
    SHIPPED_TILESET_MEMBERS
        .iter()
        .any(|member| member.eq_ignore_ascii_case(file_name))
}

/// A supplied tileset that the gamescript demonstrably does not use for this map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TileSetMismatch {
    pub class: MapClass,
    pub supplied: String,
    /// Every tileset the scripts **declare** for this map -- what to pass instead. Never empty: a
    /// mismatch is only raised when there is something concrete to name.
    ///
    /// This is the *declared* set, not the permissive one a mismatch is tested against, because a
    /// suggestion should be the precise answer even though the refusal is lenient.
    pub expected: Vec<&'static str>,
}

impl fmt::Display for TileSetMismatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} is a shipped tileset, but the gamescript reads this {} through {}. Pass one of \
             those, or a tileset of your own -- a modded name is accepted as-is",
            self.supplied,
            self.class.description(),
            self.expected.join(" or "),
        )
    }
}

/// Whether a supplied tileset contradicts what the gamescript binds this map to.
///
/// `None` means proceed, and it means that in four distinct cases: the map's extension is not one
/// the corpus classifies; the tileset is one the scripts do bind this map to; the tileset is not a
/// shipped name at all and so is presumed modded; or **the map has no binding**, in which case
/// nothing is known and nothing may be refused on. That last case is 168 of the 337 installed
/// combat maps, and silence there is deliberate -- an earlier version of this function refused the
/// gamescript's own answer and steered callers towards `tilesa01.til`, which writes slots a
/// 64-slot-atlas battle map cannot show.
pub fn tileset_mismatch(
    map_path: &std::path::Path,
    tile_set_path: &std::path::Path,
) -> Option<TileSetMismatch> {
    let class = MapClass::from_path(map_path)?;
    let supplied = tile_set_path.file_name()?.to_str()?;
    if !is_shipped_tileset(supplied) {
        return None;
    }
    let resolution = resolve_tileset(map_path)?;
    let declared = resolution.candidates();
    if declared.is_empty() {
        return None;
    }
    // Tested against the permissive union, so a tileset the engine may reach at runtime through
    // the plural selector form is never refused -- but reported against the declared set, so the
    // advice names the map's actual tileset rather than everything reachable.
    if paintable_tilesets(map_path)
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(supplied))
    {
        return None;
    }
    Some(TileSetMismatch {
        class,
        supplied: supplied.to_owned(),
        expected: declared.to_vec(),
    })
}

/// The `" on line N"` suffix of a parse error, or nothing when the caller has no line number.
///
/// The same field parsers serve [`TileSetDefinition::parse`], which reads a whole file and can
/// always say where, and the edit path, which is handed one field by a caller who cannot. Without
/// this the edit path would have to either invent a line number or duplicate the parsers, and a
/// duplicated parser is how a writer and its reader drift apart.
/// A set of terrain types written as a `.til` alternation, `6|9`.
fn join_terrain_types(set: &BTreeSet<u32>) -> String {
    set.iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join("|")
}

fn at_line(line: Option<usize>) -> String {
    line.map_or_else(String::new, |line| format!(" on line {line}"))
}

fn parse_pair(value: &str, line: Option<usize>, name: &str) -> Result<(u32, u32), TileError> {
    let fields = csv_fields(value, line)?;
    if fields.len() != 2 {
        return Err(TileError::new(format!(
            "{name}{} does not contain two values",
            at_line(line)
        )));
    }
    Ok((
        parse_u32(&fields[0], line, name)?,
        parse_u32(&fields[1], line, name)?,
    ))
}

fn parse_u32(value: &str, line: Option<usize>, name: &str) -> Result<u32, TileError> {
    value
        .trim()
        .parse()
        .map_err(|_| TileError::new(format!("invalid {name}{}: {value}", at_line(line))))
}

/// One past the number of comma-separated fields a record line may carry.
///
/// **A guard on an allocation whose size comes from the file.** Every `TERRAINTYPE=` and `TILE=`
/// row in all 26 shipped tilesets has exactly 11 fields, and this reader names at most eleven
/// positions; without a bound, a row of ten million commas turns into ten million `String`s in
/// [`csv_fields`] and ten million `Range`s in [`field_spans`], and `--describe-til` dies in the
/// allocator instead of refusing. 64 is deliberately far above 11 so a modded file with a few extra
/// trailing columns still reads, and far below anything that costs memory.
pub const MAX_RECORD_FIELDS: usize = 64;

fn csv_fields(value: &str, line: Option<usize>) -> Result<Vec<String>, TileError> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    for character in value.chars() {
        match character {
            '"' => quoted = !quoted,
            ',' if !quoted => {
                fields.push(field.trim().to_owned());
                field.clear();
                // Checked as the fields are produced, not afterwards, so the allocation is bounded
                // rather than merely reported.
                if fields.len() >= MAX_RECORD_FIELDS {
                    return Err(TileError::new(format!(
                        "comma-separated data{} carries more than {MAX_RECORD_FIELDS} fields; \
                         every shipped row has 11",
                        at_line(line)
                    )));
                }
            }
            _ => field.push(character),
        }
    }
    if quoted {
        return Err(TileError::new(format!(
            "unterminated quote in comma-separated data{}",
            at_line(line)
        )));
    }
    fields.push(field.trim().to_owned());
    Ok(fields)
}

// ---------------------------------------------------------------------------
// The write path
// ---------------------------------------------------------------------------

/// How one physical line of a `.til` ended.
///
/// **Observed in the corpus, 2026-09-18.** All 26 shipped tilesets are CRLF throughout -- 6,001
/// CRLF, zero bare LF, zero bare CR -- and all 26 end with one. The other three variants exist
/// because a modded or hand-edited file may not be, and a writer that normalised line endings
/// would rewrite every line of such a file while claiming to change one field.
///
/// **A bare `\r` is deliberately not a terminator here.** This game's own text data does include a
/// bare-CR format -- `settings.cfg` separates its records that way -- and it is tempting to accept
/// one here for symmetry. It is not accepted, for the same reason a `.til` writer does not mint a
/// field: all 26 shipped tilesets are CRLF, and **nothing establishes that `lomse.exe` would read a
/// bare-CR `.til` at all**, so accepting one would invent a capability rather than support a
/// format. Such a file collapses to a single physical line and is **refused**, which is the safe
/// direction -- and it is explicitly not the `settings.cfg` failure, where a file parsed
/// "successfully" while silently yielding nothing for 22 of 23 keys. See [`bare_cr_diagnosis`] for
/// the message it is refused with, which names the cause instead of reporting a missing key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineTerminator {
    /// `\r\n`, which is every terminator in the shipped corpus.
    CrLf,
    /// A bare `\n`.
    Lf,
    /// The last line of a file that does not end with a terminator.
    None,
}

impl LineTerminator {
    const fn as_str(self) -> &'static str {
        match self {
            Self::CrLf => "\r\n",
            Self::Lf => "\n",
            Self::None => "",
        }
    }
}

/// What one line of a `.til` is, as byte ranges into that line's own text.
///
/// Ranges rather than values: an edit replaces the span a field occupies and leaves every other
/// byte of the line -- the alignment spaces, the tabs, the trailing comment -- exactly where it
/// was. The values themselves live in the [`TileSetDefinition`], which is the parser's answer, not
/// a second one kept in step by hand.
#[derive(Debug, Clone, PartialEq, Eq)]
enum LineRecord {
    /// A comment, a blank line, a key this writer does not model, or a record line too malformed
    /// to locate fields in. Carried byte for byte and never edited.
    Carried,
    Atlas { value: Range<usize> },
    Grid { fields: Vec<Range<usize>> },
    TileSize { fields: Vec<Range<usize>> },
    TerrainType { index: u32, fields: Vec<Range<usize>> },
    Tile { index: u32, fields: Vec<Range<usize>> },
}

/// One line of a `.til`: its bytes, how it ended, and what parsing made of it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DocumentLine {
    text: String,
    terminator: LineTerminator,
    record: LineRecord,
}

/// A `.til` that can be written back.
///
/// # What this is, stated plainly
///
/// A `.til` is a **hand-written text file**: CRLF, comments, tab-and-space column alignment that no
/// two files agree on, `"happy plains" ,` with the space before the comma. None of that layout is
/// derivable from the values, so this type does not try to derive it. It keeps every line's bytes
/// and replaces **only the span of a field it is asked to change**.
///
/// That has a direct consequence for how strong the round-trip result is, and it should not be
/// oversold: **an unedited document re-encodes byte-identically by construction**, because
/// [`to_bytes`](Self::to_bytes) concatenates lines it never altered. 26 of 26 is therefore a check
/// that the line splitter and its terminators are exact, and nothing more. The claim that the
/// *field model* is faithful is a different measurement, made by
/// [`field_rebuild_audit`](Self::field_rebuild_audit), which rebuilds each field's text from the
/// typed value the parser read and can genuinely fail.
///
/// # What it never does
///
/// It never mints a line. Every edit addresses a record that is already in the file; an unknown
/// tile or terrain index is refused by name rather than appended. See the refusals on each setter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TileSetDocument {
    lines: Vec<DocumentLine>,
    definition: TileSetDefinition,
}

/// The largest atlas capacity this writer will declare. **Inclusive**: 1,024 slots are accepted,
/// 1,025 are not.
///
/// A conservative guard, not a field width. The largest shipped tileset is `tilesb01.til` at
/// 16x39 = 624 slots, and the map writer independently refuses a map cell holding a tile index of
/// 1,024 or more, so a tileset far above this could declare slots no map this project writes could
/// address. Nothing observed says the engine has a limit here at all.
pub const MAX_ATLAS_CAPACITY: u32 = 1 << 10;

/// The largest palette index a `TERRAINTYPE=` colour may name.
///
/// **Observed in the corpus.** The game's palettes are 256 entries (`artifacts/zzpal-index-map.txt`)
/// and every one of the 402 shipped `TERRAINTYPE=` colour fields is in `90..=158`. A value above
/// 255 cannot name a palette entry under any reading of an 8-bit index.
pub const MAX_PALETTE_INDEX: u32 = 255;

/// Which column of a `TERRAINTYPE=` line an edit names.
///
/// The named columns are the ones **all 26 shipped headers agree about**; the unnamed ones are the
/// four they do not, and naming them here rather than omitting them is what lets
/// [`TileSetDocument::set_terrain_field`] refuse them with the disagreement as the reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerrainColumn {
    /// Column 1, the palette index the map editor draws this terrain with.
    PaletteColor,
    /// Column 2, the free text the file calls the terrain.
    Description,
    /// Column `a`. 26 of 26 headers call it flags; see [`Passability`].
    Passability,
    /// Column `b`. 26 of 26 headers: `minimum elevation (b=1000 = 1.0 in map model)`.
    MinElevation,
    /// Column `c`. 26 of 26 headers: `maximum elevation`.
    MaxElevation,
    /// Column `f`. 26 of 26 headers: `movement cost`.
    MovementCost,
    /// Columns `d`, `e`, `g` and `h` -- always refused. See
    /// [`set_terrain_field`](TileSetDocument::set_terrain_field).
    Unnamed(char),
}

impl TerrainColumn {
    /// The column a name on a command line means, or `None` for a name no column has.
    pub fn parse(name: &str) -> Option<Self> {
        let lowered = name.trim().to_ascii_lowercase();
        Some(match lowered.as_str() {
            "color" | "colour" => Self::PaletteColor,
            "description" => Self::Description,
            "passability" | "a" => Self::Passability,
            "min-elevation" | "b" => Self::MinElevation,
            "max-elevation" | "c" => Self::MaxElevation,
            "movement-cost" | "f" => Self::MovementCost,
            "d" => Self::Unnamed('d'),
            "e" => Self::Unnamed('e'),
            "g" => Self::Unnamed('g'),
            "h" => Self::Unnamed('h'),
            _ => return None,
        })
    }

    /// Which comma-separated field of the line this column is.
    const fn field_position(self) -> usize {
        match self {
            Self::PaletteColor => 1,
            Self::Description => 2,
            Self::Passability => 3,
            Self::MinElevation => 4,
            Self::MaxElevation => 5,
            Self::Unnamed('d') => 6,
            Self::Unnamed('e') => 7,
            Self::MovementCost => 8,
            Self::Unnamed('g') => 9,
            // `h`, and any other char, which `parse` cannot produce.
            Self::Unnamed(_) => 10,
        }
    }

    /// The name this column is addressed by, for an error message that can be acted on.
    pub fn name(self) -> String {
        match self {
            Self::PaletteColor => "color".to_owned(),
            Self::Description => "description".to_owned(),
            Self::Passability => "passability".to_owned(),
            Self::MinElevation => "min-elevation".to_owned(),
            Self::MaxElevation => "max-elevation".to_owned(),
            Self::MovementCost => "movement-cost".to_owned(),
            Self::Unnamed(column) => column.to_string(),
        }
    }

    /// Every column an edit may name, so a usage message cannot drift from the parser.
    pub const ALL: [Self; 10] = [
        Self::PaletteColor,
        Self::Description,
        Self::Passability,
        Self::MinElevation,
        Self::MaxElevation,
        Self::MovementCost,
        Self::Unnamed('d'),
        Self::Unnamed('e'),
        Self::Unnamed('g'),
        Self::Unnamed('h'),
    ];
}

/// How many of a document's fields rebuild from the values the parser read out of them.
///
/// **This is the measurement that can fail**, and it is the reason the byte-identical round trip is
/// not presented as the evidence. `values_rebuilt` counts fields whose text is regenerated from a
/// typed value -- an integer through `to_string`, a neighbour column through
/// [`NeighbourConstraint::to_column`] -- and compared against the file's own characters. A column
/// spelled `9|6`, `6|6`, `06` or `+6` parses to the same value and rebuilds differently, so the
/// count is a real property of the corpus rather than an identity.
///
/// `text_fields_carried` counts the fields that carry text and have nothing to rebuild: the `LBM`
/// value, the terrain description, and the four unnamed columns. Comparing those to themselves
/// would prove nothing and they are deliberately not in `values_rebuilt`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FieldRebuildAudit {
    pub values_checked: usize,
    pub values_rebuilt: usize,
    pub text_fields_carried: usize,
    pub mismatches: Vec<String>,
}

impl FieldRebuildAudit {
    fn check(&mut self, description: &str, file_text: &str, rebuilt: &str) {
        self.values_checked += 1;
        if file_text == rebuilt {
            self.values_rebuilt += 1;
        } else {
            self.mismatches
                .push(format!("{description}: file `{file_text}`, rebuilt `{rebuilt}`"));
        }
    }
}

impl TileSetDocument {
    /// Read a `.til` keeping every byte, and the parser's answer beside it.
    ///
    /// The values come from [`TileSetDefinition::parse`] -- the same function `--view-map` and the
    /// map painter already use -- rather than from a second parser written for the writer. What
    /// this adds is **where** each value is, so one can be replaced in place.
    pub fn parse(source: &[u8]) -> Result<Self, TileError> {
        let definition = TileSetDefinition::parse(source)?;
        let text = std::str::from_utf8(source)
            .map_err(|error| TileError::new(format!("tile definition is not UTF-8: {error}")))?;
        let mut lines = Vec::new();
        for (text, terminator) in split_terminated_lines(text) {
            lines.push(DocumentLine {
                record: classify_line(text),
                text: text.to_owned(),
                terminator,
            });
        }
        let document = Self { lines, definition };
        document.check_every_record_was_located()?;
        Ok(document)
    }

    /// Every value the parser read has to have been located on exactly one line.
    ///
    /// Without this the line model and the value model could disagree silently -- a `TILE=` row the
    /// span splitter failed on would simply never be editable, and the failure would show up as
    /// "tile 391 is not declared" on a file that plainly declares it. Here it is a parse error
    /// naming the count.
    ///
    /// **It is a defensive assertion with no known reaching input**, and it is kept rather than
    /// removed because what it protects is real: `field_rebuild_audit` and the setters index
    /// `definition.tiles[index]` directly on a line's own index, which would panic rather than
    /// misreport. Every divergence it describes is currently caught earlier by
    /// [`TileSetDefinition::parse`] -- duplicate indices, an unparsable index, a row past capacity
    /// -- so it has no test that reaches it, and saying so is better than implying one exists.
    fn check_every_record_was_located(&self) -> Result<(), TileError> {
        let mut tiles = 0_usize;
        let mut terrain_types = 0_usize;
        for line in &self.lines {
            match &line.record {
                LineRecord::Tile { index, .. } => {
                    tiles += 1;
                    if !self.definition.tiles.contains_key(index) {
                        return Err(TileError::new(format!(
                            "line model found a TILE= row for tile {index} that the value model \
                             does not have"
                        )));
                    }
                }
                LineRecord::TerrainType { index, .. } => {
                    terrain_types += 1;
                    if !self.definition.terrain_types.contains_key(index) {
                        return Err(TileError::new(format!(
                            "line model found a TERRAINTYPE= row for terrain {index} that the \
                             value model does not have"
                        )));
                    }
                }
                _ => {}
            }
        }
        if tiles != self.definition.tiles.len() || terrain_types != self.definition.terrain_types.len()
        {
            return Err(TileError::new(format!(
                "line model located {tiles} tile rows and {terrain_types} terrain rows; the value \
                 model has {} and {}",
                self.definition.tiles.len(),
                self.definition.terrain_types.len()
            )));
        }
        Ok(())
    }

    /// The values this document declares.
    pub fn definition(&self) -> &TileSetDefinition {
        &self.definition
    }

    /// How many physical lines the file has.
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// This document as a complete file.
    ///
    /// For an unedited document this reproduces the input byte for byte **by construction**: the
    /// lines were never altered and their terminators were recorded, so there is nothing here that
    /// could differ. See the type's own documentation for why that is reported separately from the
    /// field audit.
    pub fn to_bytes(&self) -> Vec<u8> {
        let capacity = self
            .lines
            .iter()
            .map(|line| line.text.len() + line.terminator.as_str().len())
            .sum();
        let mut bytes = Vec::with_capacity(capacity);
        for line in &self.lines {
            bytes.extend_from_slice(line.text.as_bytes());
            bytes.extend_from_slice(line.terminator.as_str().as_bytes());
        }
        bytes
    }

    /// Rebuild every typed field's text from its value and compare it to the file's characters.
    ///
    /// See [`FieldRebuildAudit`] for what the two counts mean and why only one of them is evidence.
    pub fn field_rebuild_audit(&self) -> FieldRebuildAudit {
        let mut audit = FieldRebuildAudit::default();
        for line in &self.lines {
            match &line.record {
                LineRecord::Carried => {}
                LineRecord::Atlas { .. } => audit.text_fields_carried += 1,
                LineRecord::Grid { fields } => {
                    self.audit_pair(&mut audit, line, fields, "TILES", [
                        self.definition.columns,
                        self.definition.rows,
                    ]);
                }
                LineRecord::TileSize { fields } => {
                    self.audit_pair(&mut audit, line, fields, "TILESIZE", [
                        self.definition.tile_width,
                        self.definition.tile_height,
                    ]);
                }
                LineRecord::TerrainType { index, fields } => {
                    let terrain = &self.definition.terrain_types[index];
                    let label = format!("TERRAINTYPE {index}");
                    audit_value(&mut audit, line, fields, 0, &label, Some(terrain.index));
                    audit_value(
                        &mut audit,
                        line,
                        fields,
                        1,
                        &label,
                        Some(terrain.palette_color),
                    );
                    audit_value(
                        &mut audit,
                        line,
                        fields,
                        3,
                        &label,
                        terrain.passability.map(Passability::value),
                    );
                    audit_value(&mut audit, line, fields, 4, &label, terrain.min_elevation);
                    audit_value(&mut audit, line, fields, 5, &label, terrain.max_elevation);
                    audit_value(&mut audit, line, fields, 8, &label, terrain.movement_cost);
                    // The description and the four columns the shipped headers disagree about are
                    // text this writer carries; comparing them to themselves would prove nothing.
                    for position in [2, 6, 7, 9, 10] {
                        if fields.len() > position {
                            audit.text_fields_carried += 1;
                        }
                    }
                }
                LineRecord::Tile { index, fields } => {
                    let tile = &self.definition.tiles[index];
                    let label = format!("TILE {index}");
                    audit_value(&mut audit, line, fields, 0, &label, Some(tile.index));
                    audit_value(&mut audit, line, fields, 1, &label, Some(tile.terrain_type));
                    // **Every present column is audited, complete row or not.** Gating this on
                    // `constraints_declared` skipped all eight columns of a row with one
                    // unreadable one -- so a file could carry a malformed column and seven good
                    // ones and be reported with zero failures and eight fewer values checked. A
                    // denominator that silently shrinks on bad input is the whole defect this
                    // audit exists to avoid. The unreadable column mismatches against the `*`
                    // placeholder the parser left, which is the finding, and the seven valid ones
                    // are checked as normal because the parser did record their values.
                    for (offset, constraint) in tile.neighbours.iter().enumerate() {
                        let position = 2 + offset;
                        if let Some(span) = fields.get(position) {
                            audit.check(
                                &format!("{label} column {}", Direction::ALL[offset].column_name()),
                                &field_value(&line.text, span.clone()),
                                &constraint.to_column(),
                            );
                        }
                    }
                    audit_value(&mut audit, line, fields, 10, &label, tile.pattern_index);
                }
            }
        }
        audit
    }

    fn audit_pair(
        &self,
        audit: &mut FieldRebuildAudit,
        line: &DocumentLine,
        fields: &[Range<usize>],
        label: &str,
        values: [u32; 2],
    ) {
        for (position, value) in values.into_iter().enumerate() {
            audit_value(audit, line, fields, position, label, Some(value));
        }
    }

    /// The effective line for a key that may be written more than once.
    ///
    /// **Last assignment wins**, because that is what [`TileSetDefinition::parse`] does -- it
    /// overwrites `atlas_member`, `dimensions` and `tile_size` each time it sees the key. No
    /// shipped file writes any of the three twice; a modded one that did would otherwise be edited
    /// on a line the parser ignores, which is the quiet kind of wrong.
    fn last_line_matching(&self, matches: impl Fn(&LineRecord) -> bool) -> Option<usize> {
        self.lines
            .iter()
            .rposition(|line| matches(&line.record))
    }

    fn tile_line(&self, tile_index: u32) -> Option<usize> {
        self.lines.iter().position(|line| {
            matches!(&line.record, LineRecord::Tile { index, .. } if *index == tile_index)
        })
    }

    fn terrain_line(&self, terrain_index: u32) -> Option<usize> {
        self.lines.iter().position(|line| {
            matches!(&line.record, LineRecord::TerrainType { index, .. } if *index == terrain_index)
        })
    }

    fn line_fields(&self, line_index: usize) -> &[Range<usize>] {
        match &self.lines[line_index].record {
            LineRecord::Grid { fields }
            | LineRecord::TileSize { fields }
            | LineRecord::TerrainType { fields, .. }
            | LineRecord::Tile { fields, .. } => fields,
            LineRecord::Carried | LineRecord::Atlas { .. } => &[],
        }
    }

    /// Replace one line's text, then confirm the change through the parser before keeping it.
    ///
    /// **The edit is verified by re-reading the bytes that are about to be written**, the same
    /// shape `--set-imp-placement` and `--map-*` use. Nothing here trusts that replacing a span did
    /// what it looked like it did: if the rewritten file fails to parse, or parses to something
    /// other than what was asked, the original line is put back and the caller is refused. A writer
    /// that reported success from its own intention rather than from the reader is exactly the
    /// tautology this project has had to delete tests for.
    fn apply(
        &mut self,
        line_index: usize,
        new_text: String,
        what: &str,
        expected: impl Fn(&TileSetDefinition) -> bool,
    ) -> Result<(), TileError> {
        let original = std::mem::replace(&mut self.lines[line_index].text, new_text);
        let record = classify_line(&self.lines[line_index].text);
        self.lines[line_index].record = record;

        let outcome = TileSetDefinition::parse(&self.to_bytes());
        match outcome {
            Ok(definition) if expected(&definition) => {
                self.definition = definition;
                Ok(())
            }
            other => {
                self.lines[line_index].text = original;
                let record = classify_line(&self.lines[line_index].text);
                self.lines[line_index].record = record;
                Err(match other {
                    Ok(_) => TileError::new(format!(
                        "{what} did not read back from the bytes it would have written, so the \
                         file would have reported a change it does not contain; refusing. A value \
                         the reader normalises does this -- a description with leading or trailing \
                         spaces comes back trimmed -- so write the value the reader would read"
                    )),
                    Err(error) => TileError::new(format!(
                        "{what} would produce a file that no longer parses: {error}"
                    )),
                })
            }
        }
    }

    /// Point this tileset at a different atlas image.
    ///
    /// # What it refuses
    ///
    /// - a name that is not a `.lbm`, because all 26 shipped tilesets name one and nothing has been
    ///   observed reading anything else through `LBM=`;
    /// - a name carrying a character that would change how the line parses -- `,`, `;`, `"`, `=`,
    ///   whitespace -- or any byte outside printable ASCII;
    /// - an empty name.
    ///
    /// **It does not check that the atlas exists.** The `.lbm` lives in `pic.mpq` and this function
    /// is handed one file; `--mod-validate` is where cross-member existence is checked.
    pub fn set_atlas_member(&mut self, member: &str) -> Result<(), TileError> {
        if member.is_empty() {
            return Err(TileError::new(
                "an empty atlas name would leave the tileset with no image; refusing",
            ));
        }
        if let Some(character) = member
            .chars()
            .find(|character| !character.is_ascii_graphic() || ",;\"=".contains(*character))
        {
            return Err(TileError::new(format!(
                "atlas name {member:?} contains {character:?}, which would change how the LBM= \
                 line parses; refusing"
            )));
        }
        if member.eq_ignore_ascii_case(".lbm") {
            return Err(TileError::new(
                "atlas name \".lbm\" has no name before its extension; every shipped atlas is a \
                 named member of pic.mpq",
            ));
        }
        if !member.to_ascii_lowercase().ends_with(".lbm") {
            return Err(TileError::new(format!(
                "atlas name {member:?} is not a .lbm; all 26 shipped tilesets name one and nothing \
                 has been observed reading any other kind of atlas"
            )));
        }
        let line_index = self
            .last_line_matching(|record| matches!(record, LineRecord::Atlas { .. }))
            .ok_or_else(|| TileError::new("this tileset has no LBM= line to edit"))?;
        let LineRecord::Atlas { value } = self.lines[line_index].record.clone() else {
            unreachable!("the line was selected by its record kind")
        };
        let new_text = replace_span(&self.lines[line_index].text, value, member);
        let wanted = member.to_owned();
        self.apply(
            line_index,
            new_text,
            &format!("setting the atlas to {member:?}"),
            move |definition| definition.atlas_member == wanted,
        )
    }

    /// Change the atlas grid the slots are counted across.
    ///
    /// # What it refuses, and why each refusal is not caution
    ///
    /// - **A change to `columns` while any tile is declared.** A slot's picture is
    ///   `(index % columns, index / columns)` in the atlas, so re-columning silently repaints every
    ///   declared tile with a different image. That is the same class of edit the IMP writer refuses
    ///   when frames share pixels: it would change art the caller did not name. The count of tiles
    ///   that would move is in the message. Changing `rows` alone leaves every slot's picture where
    ///   it was.
    /// - **A capacity that would orphan declared tiles**, naming them. The parser rejects a tile at
    ///   or beyond capacity, so shrinking past one produces a file that no longer loads; the tiles
    ///   are listed rather than left for the caller to find.
    /// - Zero sides, an overflowing product, and a capacity **above** [`MAX_ATLAS_CAPACITY`].
    pub fn set_grid(&mut self, columns: u32, rows: u32) -> Result<(), TileError> {
        if columns == 0 || rows == 0 {
            return Err(TileError::new(format!(
                "atlas grid {columns}x{rows} has a zero side; a tileset with no slots can paint \
                 nothing"
            )));
        }
        let capacity = columns.checked_mul(rows).ok_or_else(|| {
            TileError::new(format!("atlas grid {columns}x{rows} overflows a capacity"))
        })?;
        if capacity > MAX_ATLAS_CAPACITY {
            return Err(TileError::new(format!(
                "atlas grid {columns}x{rows} declares {capacity} slots, past the {MAX_ATLAS_CAPACITY} \
                 this writer will declare; the largest shipped tileset declares 624"
            )));
        }
        if columns != self.definition.columns && !self.definition.tiles.is_empty() {
            return Err(TileError::new(format!(
                "changing the column count from {} to {columns} moves every one of the {} declared \
                 tiles to a different picture in {}, because a slot is (index % columns, index / \
                 columns); refusing rather than repainting tiles that were not named. Changing rows \
                 alone is safe",
                self.definition.columns,
                self.definition.tiles.len(),
                self.definition.atlas_member,
            )));
        }
        let orphaned: Vec<u32> = self
            .definition
            .tiles
            .keys()
            .copied()
            .filter(|index| *index >= capacity)
            .collect();
        if !orphaned.is_empty() {
            return Err(TileError::new(format!(
                "atlas grid {columns}x{rows} holds {capacity} slots, which leaves {} declared \
                 tile(s) outside it: {}; refusing rather than writing a tileset that no longer \
                 parses",
                orphaned.len(),
                name_list(&orphaned),
            )));
        }
        let line_index = self
            .last_line_matching(|record| matches!(record, LineRecord::Grid { .. }))
            .ok_or_else(|| TileError::new("this tileset has no TILES= line to edit"))?;
        let fields = self.line_fields(line_index).to_vec();
        let mut new_text = self.lines[line_index].text.clone();
        // Rightmost field first, so replacing one does not move the span of the other.
        new_text = replace_span(&new_text, fields[1].clone(), &rows.to_string());
        new_text = replace_span(&new_text, fields[0].clone(), &columns.to_string());
        self.apply(
            line_index,
            new_text,
            &format!("setting the atlas grid to {columns}x{rows}"),
            move |definition| definition.columns == columns && definition.rows == rows,
        )
    }

    /// The `self` column: which terrain type a slot belongs to.
    ///
    /// # What it refuses
    ///
    /// - **A tile the file does not declare**, naming it. There is no `TILE=` row to edit and this
    ///   writer does not mint one: a minted row would need eight neighbour columns nothing in the
    ///   file sources.
    /// - **A terrain type the file does not declare**, listing the ones it does. A tile pointing at
    ///   an undeclared type can never be selected and its cells read as unknown terrain.
    /// - **Moving the last tile out of a terrain type**, naming the type and its description. Every
    ///   map cell holding one of that type's tiles is read through
    ///   [`terrain_type_of_tile`](TileSetDefinition::terrain_type_of_tile); emptying the type makes
    ///   those cells unreadable, which is a change to maps the caller did not name.
    pub fn set_tile_terrain_type(&mut self, tile: u32, terrain: u32) -> Result<(), TileError> {
        let line_index = self.tile_line(tile).ok_or_else(|| self.no_such_tile(tile))?;
        self.require_declared_terrain(terrain)?;
        let previous = self.definition.tiles[&tile].terrain_type;
        if previous != terrain {
            let remaining = self
                .definition
                .tiles
                .values()
                .filter(|other| other.terrain_type == previous && other.index != tile)
                .count();
            if remaining == 0 {
                let description = self
                    .definition
                    .terrain_types
                    .get(&previous)
                    .map_or("undeclared", |definition| definition.description.as_str());
                return Err(TileError::new(format!(
                    "tile {tile} is the only tile of terrain type {previous} ({description:?}); \
                     moving it would leave that terrain with no tile at all, so every map cell \
                     holding one would read as unknown terrain. Refusing"
                )));
            }
        }
        let fields = self.line_fields(line_index).to_vec();
        let new_text = replace_span(
            &self.lines[line_index].text,
            fields[1].clone(),
            &terrain.to_string(),
        );
        self.apply(
            line_index,
            new_text,
            &format!("setting tile {tile} to terrain type {terrain}"),
            move |definition| {
                definition
                    .tiles
                    .get(&tile)
                    .is_some_and(|definition| definition.terrain_type == terrain)
            },
        )
    }

    /// One of the eight neighbour columns of one tile.
    ///
    /// # What it refuses
    ///
    /// - **A tile the file does not declare**, naming it; nothing is minted.
    /// - **A tile whose row did not declare all eight columns readably**, naming the first column
    ///   that is missing. Such a tile is already excluded from painting
    ///   ([`TileDefinition::accepts`]); writing one column of it would produce a row that looks
    ///   complete and is not.
    /// - **A constraint naming a terrain type the file does not declare**, naming the types. A
    ///   constraint can only ever be satisfied by a type some tile belongs to.
    pub fn set_tile_neighbour(
        &mut self,
        tile: u32,
        direction: Direction,
        constraint: &NeighbourConstraint,
    ) -> Result<(), TileError> {
        let line_index = self.tile_line(tile).ok_or_else(|| self.no_such_tile(tile))?;
        let definition = &self.definition.tiles[&tile];
        if !definition.constraints_declared {
            let fields = self.line_fields(line_index);
            let missing = (0..8)
                .find(|offset| {
                    fields.get(2 + offset).is_none_or(|span| {
                        let text = field_value(&self.lines[line_index].text, span.clone());
                        text.is_empty() || NeighbourConstraint::parse_column(&text).is_err()
                    })
                })
                .map_or_else(
                    || "an unreadable column".to_owned(),
                    |offset| format!("column {}", Direction::ALL[offset].column_name()),
                );
            return Err(TileError::new(format!(
                "tile {tile} did not declare all eight neighbour columns -- {missing} is missing or \
                 unreadable -- so it can never be painted; writing one column would make an \
                 incomplete row look complete. Refusing"
            )));
        }
        let undeclared: Vec<u32> = constraint
            .terrain_types()
            .into_iter()
            .filter(|terrain| !self.definition.terrain_types.contains_key(terrain))
            .collect();
        if !undeclared.is_empty() {
            return Err(TileError::new(format!(
                "constraint {} names terrain type(s) {} that this tileset does not declare; \
                 declared: {}. Refusing",
                constraint.to_column(),
                name_list(&undeclared),
                name_list(&self.definition.terrain_types.keys().copied().collect::<Vec<_>>()),
            )));
        }
        let position = Direction::ALL
            .iter()
            .position(|candidate| *candidate == direction)
            .expect("Direction::ALL contains every direction");
        let fields = self.line_fields(line_index).to_vec();
        let span = fields.get(2 + position).cloned().ok_or_else(|| {
            TileError::new(format!(
                "tile {tile}'s row has no column {}; this writer will not extend a row it did not \
                 write",
                direction.column_name()
            ))
        })?;
        let new_text = replace_span(
            &self.lines[line_index].text,
            span,
            &constraint.to_column(),
        );
        let wanted = constraint.clone();
        self.apply(
            line_index,
            new_text,
            &format!(
                "setting tile {tile} column {} to {}",
                direction.column_name(),
                constraint.to_column()
            ),
            move |definition| {
                definition
                    .tiles
                    .get(&tile)
                    .is_some_and(|tile| *tile.neighbour(direction) == wanted)
            },
        )
    }

    /// One named column of one `TERRAINTYPE=` row.
    ///
    /// # What it refuses
    ///
    /// - **Columns `d`, `e`, `g` and `h`, always, by name.** The shipped headers do not agree what
    ///   they are: 25 of 26 call `d` food and `e` ore, `tilesb01.til` calls both unused, and all 26
    ///   call `g` and `h` unused. Writing a number into a column whose meaning is a disagreement
    ///   would be minting a field, so this refuses and says which file says what.
    /// - **A terrain type the file does not declare**, naming it; nothing is minted.
    /// - **A column the row stops before.** Every shipped row has all eleven fields; a shorter one
    ///   is refused rather than extended, because extending a row means inventing values for the
    ///   columns in between.
    /// - A passability outside `0..=2`, a palette index above [`MAX_PALETTE_INDEX`], an elevation
    ///   pair that would end up with the minimum above the maximum, and a description carrying a
    ///   character that would change how the line parses.
    pub fn set_terrain_field(
        &mut self,
        terrain: u32,
        column: TerrainColumn,
        value: &str,
    ) -> Result<(), TileError> {
        if let TerrainColumn::Unnamed(name) = column {
            let disagreement = match name {
                'd' => "25 of the 26 shipped headers call column d food; tilesb01.til calls it unused",
                'e' => "25 of the 26 shipped headers call column e ore; tilesb01.til calls it unused",
                _ => "all 26 shipped headers call this column unused",
            };
            return Err(TileError::new(format!(
                "column {name} has no sourced meaning -- {disagreement} -- so writing a value into \
                 it would be minting a field. Refusing"
            )));
        }
        let line_index = self
            .terrain_line(terrain)
            .ok_or_else(|| self.no_such_terrain(terrain))?;
        let fields = self.line_fields(line_index).to_vec();
        let position = column.field_position();
        let span = fields.get(position).cloned().ok_or_else(|| {
            TileError::new(format!(
                "terrain type {terrain}'s row stops after {} field(s) and has no {} column; this \
                 writer will not extend a row it did not write",
                fields.len(),
                column.name()
            ))
        })?;

        let replacement = match column {
            TerrainColumn::Description => {
                if value.trim().is_empty() {
                    return Err(TileError::new(
                        "an empty description would leave the terrain type unnamed; refusing",
                    ));
                }
                if let Some(character) = value
                    .chars()
                    .find(|character| ",;\"".contains(*character) || character.is_control())
                {
                    return Err(TileError::new(format!(
                        "description {value:?} contains {character:?}, which would change how the \
                         TERRAINTYPE= line parses; refusing"
                    )));
                }
                // Quoting is the file's, not this writer's: every shipped description is quoted,
                // and a file that wrote one bare keeps it bare.
                if field_text(&self.lines[line_index].text, span.clone()).starts_with('"') {
                    format!("\"{value}\"")
                } else {
                    value.to_owned()
                }
            }
            _ => {
                let number = parse_u32(value, None, &column.name())?;
                self.check_terrain_number(terrain, column, number)?;
                number.to_string()
            }
        };

        let new_text = replace_span(&self.lines[line_index].text, span, &replacement);
        let wanted = replacement.clone();
        let column_name = column.name();
        self.apply(
            line_index,
            new_text,
            &format!("setting terrain type {terrain} {column_name} to {value:?}"),
            move |definition| {
                let Some(read_back) = definition.terrain_types.get(&terrain) else {
                    return false;
                };
                match column {
                    TerrainColumn::PaletteColor => {
                        read_back.palette_color.to_string() == wanted
                    }
                    TerrainColumn::Description => {
                        read_back.description == wanted.trim_matches('"')
                    }
                    TerrainColumn::Passability => read_back
                        .passability
                        .is_some_and(|passability| passability.value().to_string() == wanted),
                    TerrainColumn::MinElevation => read_back
                        .min_elevation
                        .is_some_and(|value| value.to_string() == wanted),
                    TerrainColumn::MaxElevation => read_back
                        .max_elevation
                        .is_some_and(|value| value.to_string() == wanted),
                    TerrainColumn::MovementCost => read_back
                        .movement_cost
                        .is_some_and(|value| value.to_string() == wanted),
                    TerrainColumn::Unnamed(_) => false,
                }
            },
        )
    }

    /// Always refused, by name: `TILESIZE=`.
    ///
    /// **Observed in the corpus:** all 26 shipped tilesets declare `32, 32`, and the tile geometry
    /// the map renderer and the projection in `tools/map_projection.py` use is 32x32 throughout.
    /// Nothing in this project has observed the engine read this line at all, so there is no
    /// evidence that a different value would be honoured rather than ignored or crashed on. A
    /// writer that emitted `64, 64` would be minting a capability.
    pub fn set_tile_size(&mut self, width: u32, height: u32) -> Result<(), TileError> {
        Err(TileError::new(format!(
            "refusing to declare a {width}x{height} tile: all 26 shipped tilesets declare 32x32 and \
             no observation in this project says the engine reads TILESIZE= at all, so a different \
             value would be an invented capability rather than an edit"
        )))
    }

    /// Always refused, by name: the trailing `index` column of a `TILE=` row.
    ///
    /// The column is a pattern id shared across terrain blocks, and it is **demonstrably not
    /// reliable** -- tile 2 of `tilesb01.til` carries 6 where its pattern is plainly 2, and water's
    /// tile 50 carries 1 where tile 2's analogue is 2. Nothing in this project decides anything
    /// from it. Writing a value into a column whose rule is not known would be minting a field.
    pub fn set_tile_pattern_index(&mut self, tile: u32, pattern: u32) -> Result<(), TileError> {
        Err(TileError::new(format!(
            "refusing to set tile {tile}'s pattern column to {pattern}: the column's rule is not \
             known -- tilesb01.til's tile 2 carries 6 where its pattern is 2, and its tile 50 \
             carries 1 -- and nothing in this project reads it"
        )))
    }

    /// The bounds each named numeric column is written under, kept out of the setter so each
    /// refusal reads as its own sentence.
    fn check_terrain_number(
        &self,
        terrain: u32,
        column: TerrainColumn,
        number: u32,
    ) -> Result<(), TileError> {
        match column {
            TerrainColumn::Passability if number > 2 => Err(TileError::new(format!(
                "passability {number} is outside the vocabulary the shipped headers declare -- \
                 tilesb01.til says 0=land, 1=water, 2=impassable and the other 25 say 0=land, \
                 1=water -- so its meaning is not sourced. Refusing"
            ))),
            TerrainColumn::PaletteColor if number > MAX_PALETTE_INDEX => {
                Err(TileError::new(format!(
                    "palette index {number} is past {MAX_PALETTE_INDEX}; the game's palettes hold \
                     256 entries and every shipped terrain colour is in 90..=158"
                )))
            }
            TerrainColumn::MinElevation | TerrainColumn::MaxElevation => {
                let definition = &self.definition.terrain_types[&terrain];
                let (minimum, maximum) = if column == TerrainColumn::MinElevation {
                    (Some(number), definition.max_elevation)
                } else {
                    (definition.min_elevation, Some(number))
                };
                match (minimum, maximum) {
                    (Some(minimum), Some(maximum)) if minimum > maximum => {
                        Err(TileError::new(format!(
                            "elevation range {minimum}..{maximum} for terrain type {terrain} is \
                             inverted; no shipped row writes one and nothing says what the engine \
                             would do with it. Refusing"
                        )))
                    }
                    _ => Ok(()),
                }
            }
            _ => Ok(()),
        }
    }

    fn require_declared_terrain(&self, terrain: u32) -> Result<(), TileError> {
        if self.definition.terrain_types.contains_key(&terrain) {
            return Ok(());
        }
        Err(self.no_such_terrain(terrain))
    }

    fn no_such_tile(&self, tile: u32) -> TileError {
        TileError::new(format!(
            "this tileset declares no TILE= row for slot {tile}, and this writer does not mint one: \
             a minted row would need eight neighbour columns that nothing in the file sources. \
             {} tile(s) are declared, from {} to {}",
            self.definition.tiles.len(),
            self.definition
                .tiles
                .keys()
                .next()
                .map_or_else(|| "none".to_owned(), u32::to_string),
            self.definition
                .tiles
                .keys()
                .next_back()
                .map_or_else(|| "none".to_owned(), u32::to_string),
        ))
    }

    fn no_such_terrain(&self, terrain: u32) -> TileError {
        TileError::new(format!(
            "this tileset declares no TERRAINTYPE= row for {terrain}, and this writer does not mint \
             one; declared: {}",
            name_list(
                &self
                    .definition
                    .terrain_types
                    .keys()
                    .copied()
                    .collect::<Vec<_>>()
            ),
        ))
    }
}

/// One audited integer field, or nothing when the row stops before it.
///
/// A field that exists, is non-empty, and whose value the parser recorded as absent is a
/// **mismatch**, not a skip: it means the parser could not read a column the file wrote, which is
/// precisely the silent loss this audit exists to find.
fn audit_value(
    audit: &mut FieldRebuildAudit,
    line: &DocumentLine,
    fields: &[Range<usize>],
    position: usize,
    label: &str,
    value: Option<u32>,
) {
    let Some(span) = fields.get(position) else {
        return;
    };
    let text = field_value(&line.text, span.clone());
    match value {
        Some(value) => audit.check(&format!("{label} field {position}"), &text, &value.to_string()),
        None if text.is_empty() => {}
        None => {
            audit.values_checked += 1;
            audit.mismatches.push(format!(
                "{label} field {position}: file `{text}`, the parser read no value"
            ));
        }
    }
}

/// A list of numbers for an error message, capped so a refusal stays readable.
fn name_list(values: &[u32]) -> String {
    const SHOWN: usize = 12;
    let shown = values
        .iter()
        .take(SHOWN)
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    if values.len() > SHOWN {
        format!("{shown} and {} more", values.len() - SHOWN)
    } else {
        shown
    }
}

/// A line with one span replaced, every other byte untouched.
fn replace_span(text: &str, span: Range<usize>, replacement: &str) -> String {
    let mut edited = text.to_owned();
    edited.replace_range(span, replacement);
    edited
}

/// The characters a field occupies, quotes and all.
fn field_text(text: &str, span: Range<usize>) -> &str {
    &text[span]
}

/// A field's value the way [`csv_fields`] produces it: quote characters dropped, then trimmed.
fn field_value(text: &str, span: Range<usize>) -> String {
    text[span]
        .chars()
        .filter(|character| *character != '"')
        .collect::<String>()
        .trim()
        .to_owned()
}

/// The refusal a bare-CR file earns, or `None` when its line endings are not the problem.
///
/// **Only consulted once parsing has already failed.** A file with a stray `\r` inside a line that
/// parses anyway is left alone; this exists to replace a misleading message, not to add a rule.
///
/// The misleading message is the point. A `.til` whose records are separated by bare `\r` collapses
/// to one physical line -- `str::lines` splits on `\n` -- so the reader sees `LBM=...` followed by a
/// `;`-comment and reports "tile definition has no TILES dimensions". That names a key, and the
/// key is present. Refusing the file is correct (see [`LineTerminator`]); refusing it for a reason
/// that sends the reader to the wrong line is not.
fn bare_cr_diagnosis(source: &[u8]) -> Option<TileError> {
    let bare = source
        .iter()
        .enumerate()
        .filter(|(index, byte)| **byte == b'\r' && source.get(index + 1) != Some(&b'\n'))
        .count();
    if bare == 0 {
        return None;
    }
    Some(TileError::new(format!(
        "this file separates {bare} record(s) with a bare CR and no LF; a .til is read a line at a \
         time and a bare CR is not a line break, so the whole file reads as one line. All 26 \
         shipped tilesets are CRLF, and nothing in this project has observed the engine reading any \
         other layout, so this is refused rather than guessed at"
    )))
}

/// Split text into lines, recording how each one ended.
///
/// **This is the one splitter, and both readers use it.** [`TileSetDefinition::parse`] iterates it
/// for the values and [`TileSetDocument::parse`] for the bytes. Two splitters -- `str::lines` for
/// one and a hand-rolled walk for the other -- is precisely how a file comes to parse and then fail
/// to round-trip, or to round-trip bytes the value model never saw.
///
/// It splits on `\n` and treats one preceding `\r` as part of the terminator, which is exactly
/// `str::lines`'s rule. That equivalence is deliberate and load-bearing: the values were read with
/// `str::lines` before this existed, and any divergence would put the line model and the value
/// model on different ideas of where a line ends. A bare `\r` is therefore **not** a line break --
/// see [`LineTerminator`] for why accepting one would be minting, and [`bare_cr_diagnosis`] for how
/// such a file is refused.
fn split_terminated_lines(text: &str) -> Vec<(&str, LineTerminator)> {
    let mut lines = Vec::new();
    let mut rest = text;
    // `\r` and `\n` are ASCII, so every index here is a character boundary.
    while let Some(position) = rest.find('\n') {
        let (line, remainder) = rest.split_at(position);
        rest = &remainder['\n'.len_utf8()..];
        match line.strip_suffix('\r') {
            Some(stripped) => lines.push((stripped, LineTerminator::CrLf)),
            None => lines.push((line, LineTerminator::Lf)),
        }
    }
    if !rest.is_empty() {
        lines.push((rest, LineTerminator::None));
    }
    lines
}

/// What one line is, and where its fields are.
///
/// Comments are cut at the **first** `;`, including one inside a quoted description -- because that
/// is what [`TileSetDefinition::parse`] does, and a line model that disagreed with the value model
/// about where a line ends would edit bytes the parser never read. No shipped description contains
/// a `;`.
fn classify_line(text: &str) -> LineRecord {
    let body_end = text.find(';').unwrap_or(text.len());
    let Some(equals) = text[..body_end].find('=') else {
        return LineRecord::Carried;
    };
    let key = text[..equals].trim().to_ascii_uppercase();
    let value = equals + 1..body_end;
    let index_of = |fields: &[Range<usize>]| -> Option<u32> {
        let span = fields.first()?;
        field_value(text, span.clone()).parse().ok()
    };
    match key.as_str() {
        "LBM" => LineRecord::Atlas {
            value: trimmed_span(text, value),
        },
        "TILES" | "TILESIZE" => {
            let Some(fields) = field_spans(text, value) else {
                return LineRecord::Carried;
            };
            if fields.len() != 2 {
                return LineRecord::Carried;
            }
            if key == "TILES" {
                LineRecord::Grid { fields }
            } else {
                LineRecord::TileSize { fields }
            }
        }
        "TERRAINTYPE" | "TILE" => {
            let Some(fields) = field_spans(text, value) else {
                return LineRecord::Carried;
            };
            let Some(index) = index_of(&fields) else {
                return LineRecord::Carried;
            };
            if key == "TILE" {
                LineRecord::Tile { index, fields }
            } else {
                LineRecord::TerrainType { index, fields }
            }
        }
        _ => LineRecord::Carried,
    }
}

/// The comma-separated fields of a value region, as trimmed spans.
///
/// Quote state is tracked so a comma inside a description is not a separator, matching
/// [`csv_fields`]. An unterminated quote is not an error here: the value model has already rejected
/// such a line for the two record kinds where it matters.
fn field_spans(text: &str, value: Range<usize>) -> Option<Vec<Range<usize>>> {
    let mut spans = Vec::new();
    let mut start = value.start;
    let mut quoted = false;
    for (offset, character) in text[value.clone()].char_indices() {
        let position = value.start + offset;
        match character {
            '"' => quoted = !quoted,
            ',' if !quoted => {
                spans.push(trimmed_span(text, start..position));
                start = position + ','.len_utf8();
                // The same bound [`csv_fields`] applies, for the same reason and in the same place:
                // as the spans are produced. A line past it is carried rather than located, which
                // makes it uneditable -- and the value model has already refused the file anyway.
                if spans.len() >= MAX_RECORD_FIELDS {
                    return None;
                }
            }
            _ => {}
        }
    }
    spans.push(trimmed_span(text, start..value.end));
    Some(spans)
}

/// A span narrowed to the non-whitespace it contains.
fn trimmed_span(text: &str, span: Range<usize>) -> Range<usize> {
    let slice = &text[span.clone()];
    let start = span.start + (slice.len() - slice.trim_start().len());
    let end = span.end - (slice.len() - slice.trim_end().len());
    start..end.max(start)
}

#[cfg(test)]
mod tests {
    use super::{
        Direction, MAX_ATLAS_CAPACITY, MAX_PALETTE_INDEX, MAX_RECORD_FIELDS,
        NeighbourConstraint, Neighbourhood, Passability, TerrainColumn, TileChoice, TileError,
        TileSelector, TileSetDefinition, TileSetDocument,
    };
    use std::collections::BTreeSet;

    fn one_of(values: &[u32]) -> NeighbourConstraint {
        NeighbourConstraint::OneOf(values.iter().copied().collect::<BTreeSet<u32>>())
    }

    fn none_of(values: &[u32]) -> NeighbourConstraint {
        NeighbourConstraint::NoneOf(values.iter().copied().collect::<BTreeSet<u32>>())
    }

    #[test]
    fn parses_atlas_terrain_and_tile_relationships() {
        let source = br#"
LBM=tilesb01.lbm
TILES= 16, 39
TILESIZE= 32, 32
TERRAINTYPE= 6, 125, "plains", 0, 0, 9999
TILE= 619, 6, 9, 6, 6
; TILE= 700, 6, commented out
"#;

        let tile_set = TileSetDefinition::parse(source).unwrap();

        assert_eq!(tile_set.atlas_member, "tilesb01.lbm");
        assert_eq!(tile_set.atlas_capacity(), 624);
        assert_eq!(tile_set.tile_width, 32);
        assert_eq!(tile_set.terrain_types[&6].description, "plains");
        assert_eq!(tile_set.tiles[&619].terrain_type, 6);
        assert!(!tile_set.tiles.contains_key(&700));
    }

    /// Every one of the three constraint forms, on a line shaped like the shipped file's but with
    /// a **different value in every column**, so a parser that read one column into another is
    /// caught rather than passing on a symmetric row.
    #[test]
    fn parses_all_three_neighbour_constraint_forms_into_the_right_columns() {
        let source = br#"
LBM=x.lbm
TILES= 4, 2
TILESIZE= 8, 8
TILE= 0, 5, 1, ~2, 3|4, *, ~5|6, 7, 8, ~9, 77
"#;
        let tile_set = TileSetDefinition::parse(source).unwrap();
        let tile = &tile_set.tiles[&0];

        assert_eq!(tile.index, 0, "the key is the atlas slot");
        assert_eq!(tile.terrain_type, 5, "the `self` column");
        assert_eq!(tile.pattern_index, Some(77), "the trailing column is not the slot");

        assert_eq!(*tile.neighbour(Direction::North), one_of(&[1]));
        assert_eq!(*tile.neighbour(Direction::NorthEast), none_of(&[2]));
        assert_eq!(*tile.neighbour(Direction::East), one_of(&[3, 4]));
        assert_eq!(*tile.neighbour(Direction::SouthEast), NeighbourConstraint::Any);
        assert_eq!(*tile.neighbour(Direction::South), none_of(&[5, 6]));
        assert_eq!(*tile.neighbour(Direction::SouthWest), one_of(&[7]));
        assert_eq!(*tile.neighbour(Direction::West), one_of(&[8]));
        assert_eq!(*tile.neighbour(Direction::NorthWest), none_of(&[9]));
    }

    /// The `TERRAINTYPE=` columns the file's own header names, and the ones it does not.
    ///
    /// Every value differs from every other so a column read at the wrong offset is caught. The
    /// four unnamed columns stay text: `tilesb01.til` marks `d` and `e` unused while
    /// `tilesa01.til` calls them food and ore, so this writer must not pick a side.
    #[test]
    fn parses_the_terrain_type_columns_the_header_names_and_leaves_the_rest_unnamed() {
        let source = br#"
LBM=x.lbm
TILES= 4, 2
TILESIZE= 8, 8
TERRAINTYPE= 3, 44, "marsh", 1, 250, 8888, 61, 62, 7, 63, 64
TERRAINTYPE= 4, 45, "cliff", 2, 0, 1, 0, 0, 9999, 0, 0
TERRAINTYPE= 5, 46, "short"
"#;
        let tile_set = TileSetDefinition::parse(source).unwrap();

        let marsh = &tile_set.terrain_types[&3];
        assert_eq!(marsh.description, "marsh");
        assert_eq!(marsh.palette_color, 44);
        assert_eq!(marsh.passability, Some(Passability::Water));
        assert_eq!(marsh.min_elevation, Some(250));
        assert_eq!(marsh.max_elevation, Some(8888));
        assert_eq!(marsh.movement_cost, Some(7));
        assert_eq!(marsh.unnamed_fields, vec!["61", "62", "63", "64"]);

        assert_eq!(
            tile_set.terrain_types[&4].passability,
            Some(Passability::Impassable)
        );
        assert_eq!(tile_set.terrain_types[&4].movement_cost, Some(9999));

        // A line that stops after the description is read, not rejected: the shipped tilesets are
        // already two different shapes, so a short line is data, not corruption.
        let short = &tile_set.terrain_types[&5];
        assert_eq!(short.passability, None);
        assert_eq!(short.movement_cost, None);
        assert!(short.unnamed_fields.is_empty());
    }

    /// An unrecognised passability value is carried, not rejected and not coerced.
    ///
    /// `tilesb01.til` documents 0, 1 and 2; `tilesa01.til` documents only 0 and 1. A third file
    /// with a fourth value must not be silently read as land.
    #[test]
    fn an_unknown_passability_value_is_carried_rather_than_coerced() {
        let source = br#"
LBM=x.lbm
TILES= 4, 2
TILESIZE= 8, 8
TERRAINTYPE= 0, 1, "odd", 7, 0, 0, 0, 0, 1, 0, 0
"#;
        let tile_set = TileSetDefinition::parse(source).unwrap();
        assert_eq!(
            tile_set.terrain_types[&0].passability,
            Some(Passability::Unrecognised(7))
        );
    }

    /// An off-map neighbour satisfies every constraint, including a negated one.
    ///
    /// This is an **assumption**, not a measurement -- the `terrainrings` blobs are all interior,
    /// so no artifact says what the engine reads past an edge. The test pins the assumption so a
    /// change to it is a visible change rather than a quiet one.
    #[test]
    fn an_off_map_neighbour_satisfies_every_constraint_form() {
        assert!(NeighbourConstraint::Any.accepts(None));
        assert!(one_of(&[1, 2]).accepts(None));
        assert!(none_of(&[1, 2]).accepts(None));
        // And a present neighbour is still judged.
        assert!(one_of(&[1, 2]).accepts(Some(2)));
        assert!(!one_of(&[1, 2]).accepts(Some(3)));
        assert!(!none_of(&[1, 2]).accepts(Some(2)));
        assert!(none_of(&[1, 2]).accepts(Some(3)));
    }

    /// The eight column names map to eight **distinct** offsets that are consistent as a system:
    /// each is the negation of its opposite, and no two share a step.
    ///
    /// The mapping itself is derived against the engine's saved maps -- see
    /// [`Direction::offset`] -- and pinned end to end by the map tests. This holds the shape.
    #[test]
    fn the_eight_directions_are_distinct_and_opposite_in_pairs() {
        let offsets: BTreeSet<(i32, i32)> =
            Direction::ALL.iter().map(|d| d.offset()).collect();
        assert_eq!(offsets.len(), 8);
        for (left, right) in [
            (Direction::North, Direction::South),
            (Direction::East, Direction::West),
            (Direction::NorthEast, Direction::SouthWest),
            (Direction::NorthWest, Direction::SouthEast),
        ] {
            let (lx, ly) = left.offset();
            let (rx, ry) = right.offset();
            assert_eq!((lx, ly), (-rx, -ry), "{left:?} must oppose {right:?}");
        }
        // North is towards decreasing y, as everywhere else in this writer.
        assert_eq!(Direction::North.offset(), (0, -1));
    }

    /// Selection reports the four outcomes it actually has, and the seeded selector stays inside
    /// the candidate set.
    #[test]
    fn selection_distinguishes_unique_kept_drawn_and_no_candidate() {
        let source = br#"
LBM=x.lbm
TILES= 4, 2
TILESIZE= 8, 8
TILE= 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0
TILE= 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0
TILE= 2, 1, 2, *, 1, *, 1, *, 1, *, 2
TILE= 3, 9, 9, 9, 9, 9, 9, 9, 9, 9, 3
"#;
        let tile_set = TileSetDefinition::parse(source).unwrap();
        let all_grass = Neighbourhood::from_lookup(|_, _| Some(1));

        // Two interior tiles match a fully surrounded cell, and the cell holds neither: drawn.
        assert_eq!(
            tile_set.select_tile(1, &all_grass, Some(2), TileSelector::LowestSlot, (0, 0)),
            TileChoice::Drawn { chosen: 0, candidates: vec![0, 1] }
        );
        // The same cell already holding a valid one keeps it, including the non-lowest.
        assert_eq!(
            tile_set.select_tile(1, &all_grass, Some(1), TileSelector::LowestSlot, (0, 0)),
            TileChoice::Kept { tile: 1, candidates: vec![0, 1] }
        );
        assert!(
            tile_set
                .select_tile(1, &all_grass, Some(1), TileSelector::LowestSlot, (0, 0))
                .is_reproducible(),
            "keeping a valid tile is what the engine was observed doing, not a tie-break"
        );
        assert!(
            !tile_set
                .select_tile(1, &all_grass, Some(2), TileSelector::LowestSlot, (0, 0))
                .is_reproducible(),
            "a newly painted interior is the random draw and must not claim to be reproducible"
        );
        // One tile matches a cell with terrain 2 to its north. A unique candidate wins even if the
        // cell is holding something else entirely.
        let mut north_is_two = all_grass;
        north_is_two.set(Direction::North, Some(2));
        assert_eq!(
            tile_set.select_tile(1, &north_is_two, Some(0), TileSelector::LowestSlot, (0, 0)),
            TileChoice::Unique(2)
        );
        // Terrain 9's only tile needs all-9 neighbours, and gets none.
        assert_eq!(
            tile_set.select_tile(9, &all_grass, None, TileSelector::LowestSlot, (0, 0)),
            TileChoice::NoCandidate
        );
        // A seeded choice is still a candidate, and the same seed and cell give the same tile.
        for seed in 0..32 {
            let choice =
                tile_set.select_tile(1, &all_grass, Some(2), TileSelector::Seeded(seed), (3, 4));
            let tile = choice.tile().unwrap();
            assert!([0, 1].contains(&tile), "seed {seed} chose {tile}");
            assert_eq!(
                tile_set
                    .select_tile(1, &all_grass, Some(2), TileSelector::Seeded(seed), (3, 4))
                    .tile(),
                Some(tile)
            );
        }
    }

    /// An off-map neighbour is read as the cell's own terrain when that narrows the choice.
    ///
    /// **This is the phantom-coastline fix.** Read open, an off-map neighbour satisfies even a
    /// negated constraint, so the one-sided boundary tiles compete with the interior family along
    /// every edge and a lowest-slot tie-break takes the *most* wrong legal option. Tile 2 here
    /// asserts terrain 2 to the north; a cell on the top row must not take it just because there is
    /// nothing up there to contradict it.
    #[test]
    fn a_map_edge_is_read_closed_so_a_boundary_tile_cannot_win_on_absence() {
        let source = br#"
LBM=x.lbm
TILES= 4, 2
TILESIZE= 8, 8
TILE= 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0
TILE= 2, 1, 2, *, 1, *, 1, *, 1, *, 2
"#;
        let tile_set = TileSetDefinition::parse(source).unwrap();
        // North, north-east and north-west off the map; everything else is the cell's own terrain.
        let mut edge = Neighbourhood::from_lookup(|_, _| Some(1));
        for direction in [Direction::North, Direction::NorthEast, Direction::NorthWest] {
            edge.set(direction, None);
        }
        // Open, tile 2 is legal: its `n = 2` is satisfied by there being no north at all.
        assert_eq!(tile_set.candidates(1, &edge), vec![0, 2]);
        // Closed, only the interior tile survives, and that is what selection uses.
        assert_eq!(tile_set.candidates(1, &edge.closed_with(1)), vec![0]);
        assert_eq!(
            tile_set.select_tile(1, &edge, None, TileSelector::LowestSlot, (0, 0)),
            TileChoice::Unique(0)
        );
        // Closing must never invent a tile the open reading rejected: closed candidates are a
        // subset. With no tile that survives closing, the open set is used rather than refusing.
        let only_boundary = TileSetDefinition::parse(
            b"LBM=x.lbm\nTILES=4,2\nTILESIZE=8,8\nTILE= 2, 1, 2, *, 1, *, 1, *, 1, *, 2\n",
        )
        .unwrap();
        assert!(only_boundary.candidates(1, &edge.closed_with(1)).is_empty());
        assert_eq!(
            only_boundary.select_tile(1, &edge, None, TileSelector::LowestSlot, (0, 0)),
            TileChoice::Unique(2)
        );
    }

    /// **A malformed constraint column must not make a tileset unopenable.**
    ///
    /// Reading the eight neighbour columns is a new capability, and a new capability that can reject
    /// a file the old code accepted is a regression for `--view-map` and `--export-map-preview`,
    /// which read only the slot and the `self` column. All 26 shipped tilesets parse, so this can
    /// only bite a modded or hand-edited file -- exactly the audience that would notice.
    ///
    /// So an unreadable neighbour column, an unreadable trailing index and an unreadable
    /// `TERRAINTYPE` attribute are all recorded as *absent* and leave the tile unpaintable. What
    /// stays a hard error is anything the renderer genuinely needs: the atlas name, the grid
    /// dimensions, the tile size, the slot number and the `self` column.
    #[test]
    fn malformed_columns_leave_a_tile_unpaintable_but_the_tileset_readable() {
        let source = br#"
LBM=x.lbm
TILES= 4, 2
TILESIZE= 8, 8
TERRAINTYPE= 2, 9, "stone", nonsense, 100, 200, 0, 0, oops, 0, 0
TILE= 0, 2, 2, 2, 2, 2, 2, 2, 2, 2, 0
TILE= 1, 2, 2, 2, ~, 2, 2, 2, 2, 2, 5
TILE= 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, notanumber
"#;
        let tile_set = TileSetDefinition::parse(source).unwrap();

        // The file opens, and everything the renderer needs is there.
        assert_eq!(tile_set.atlas_capacity(), 8);
        assert_eq!(tile_set.tiles.len(), 3);
        for slot in 0..=2 {
            assert_eq!(tile_set.terrain_type_of_tile(slot), Some(2));
        }
        // The unreadable attributes are absent, not guessed and not fatal.
        let stone = &tile_set.terrain_types[&2];
        assert_eq!(stone.description, "stone");
        assert_eq!(stone.passability, None);
        assert_eq!(stone.movement_cost, None);
        assert_eq!(stone.min_elevation, Some(100));

        // Tile 1's bad constraint makes it unpaintable; tile 2's bad trailing column does not,
        // because nothing decides anything from that column.
        let all_stone = Neighbourhood::from_lookup(|_, _| Some(2));
        assert!(!tile_set.tiles[&1].constraints_are_complete());
        assert!(tile_set.tiles[&2].constraints_are_complete());
        assert_eq!(tile_set.tiles[&2].pattern_index, None);
        assert_eq!(tile_set.tiles[&1].pattern_index, Some(5));
        assert_eq!(tile_set.candidates(2, &all_stone), vec![0, 2]);

        // And the things the renderer needs are still hard errors.
        for (bad, expected) in [
            (&b"TILES=4,2\nTILESIZE=8,8\nTILE=0,1\n"[..], "tile definition has no LBM"),
            (&b"LBM=x.lbm\nTILESIZE=8,8\nTILE=0,1\n"[..], "tile definition has no TILES dimensions"),
        ] {
            assert_eq!(
                TileSetDefinition::parse(bad).unwrap_err().to_string(),
                expected
            );
        }
        assert!(
            TileSetDefinition::parse(
                b"LBM=x.lbm\nTILES=4,2\nTILESIZE=8,8\nTILE=oops,1,1,1,1,1,1,1,1,1,0\n"
            )
            .unwrap_err()
            .to_string()
            .contains("invalid tile index")
        );
    }

    /// A row that does not declare all eight neighbour columns is parsed, flagged, and unpaintable.
    ///
    /// **Every `TILE=` row in all 26 shipped tilesets has exactly 11 fields** -- 4,043 rows,
    /// counted 2026-09-17 -- so nothing shipped needs the leniency this replaces. Padding a short
    /// row with `*` made it a wildcard selectable for any neighbourhood, which is the opposite of
    /// what a missing declaration means.
    #[test]
    fn a_truncated_tile_row_is_inspectable_but_never_paintable() {
        let source = br#"
LBM=x.lbm
TILES= 4, 2
TILESIZE= 8, 8
TILE= 0, 2, 2, 2, 2, 2, 2, 2, 2, 2, 0
TILE= 1, 2
TILE= 2, 2, 2, 2, 2, 2, 2
"#;
        let tile_set = TileSetDefinition::parse(source).unwrap();

        // All three are readable, so a malformed file still renders through `--view-map`.
        assert_eq!(tile_set.tiles.len(), 3);
        assert_eq!(tile_set.terrain_type_of_tile(1), Some(2));
        assert_eq!(tile_set.terrain_type_of_tile(2), Some(2));
        assert!(tile_set.tiles[&0].constraints_are_complete());
        assert!(!tile_set.tiles[&1].constraints_are_complete());
        assert!(!tile_set.tiles[&2].constraints_are_complete());

        // Only the complete one can be selected, for any neighbourhood at all.
        let all_stone = Neighbourhood::from_lookup(|_, _| Some(2));
        assert_eq!(tile_set.candidates(2, &all_stone), vec![0]);
        assert!(!tile_set.tiles[&1].accepts(&all_stone));
        assert!(!tile_set.tiles[&2].accepts(&all_stone));
        // And not even against a neighbourhood its written columns would have matched.
        let mixed = Neighbourhood::from_lookup(|dx, _| if dx > 0 { Some(7) } else { Some(2) });
        assert!(!tile_set.tiles[&2].accepts(&mixed));
    }

    #[test]
    fn rejects_tiles_outside_the_declared_atlas() {
        let source = b"LBM=x.lbm\nTILES=1,1\nTILESIZE=32,32\nTILE=1,0\n";

        assert_eq!(
            TileSetDefinition::parse(source).unwrap_err().to_string(),
            "tile 1 exceeds declared atlas capacity 1"
        );
    }

    /// Map classification, including the **case split** the installed corpus actually has.
    ///
    /// The `map/` directory ships 172 `.smp` and 165 `.SMP`. A case-sensitive classifier would
    /// call 165 combat maps unknown, so an uppercase input is the input that makes this fail.
    #[test]
    fn map_class_follows_the_extension_case_insensitively() {
        use super::MapClass;
        use std::path::Path;

        assert_eq!(MapClass::from_extension("smp"), Some(MapClass::Combat));
        assert_eq!(MapClass::from_extension("SMP"), Some(MapClass::Combat));
        assert_eq!(MapClass::from_extension("Smp"), Some(MapClass::Combat));
        for lower in ["scn", "lgd", "map"] {
            assert_eq!(MapClass::from_extension(lower), Some(MapClass::World));
            assert_eq!(
                MapClass::from_extension(&lower.to_uppercase()),
                Some(MapClass::World)
            );
        }
        assert_eq!(MapClass::from_extension("til"), None);
        assert_eq!(MapClass::from_extension(""), None);

        assert_eq!(
            MapClass::from_path(Path::new("map/AIBLDG01.SMP")),
            Some(MapClass::Combat)
        );
        assert_eq!(
            MapClass::from_path(Path::new("map/URAK.scn")),
            Some(MapClass::World)
        );
        assert_eq!(MapClass::from_path(Path::new("URAK")), None);
    }

    /// The binding table's own invariants, which are what make the binary search correct and the
    /// data reviewable.
    ///
    /// **This is the test that a regenerated table has to pass.** The table is extracted from
    /// `gs.mpq` by a script, so the thing that can go wrong is not a typo in one entry but a
    /// malformed regeneration: unsorted keys, an uppercase key the case-insensitive lookup would
    /// then miss, a duplicate map, an empty candidate list, or a candidate list that is itself
    /// unsorted or duplicated.
    #[test]
    fn the_binding_table_is_sorted_lowercase_and_free_of_empty_or_duplicate_entries() {
        use super::COMBAT_TILESET_BINDINGS;

        assert!(
            !COMBAT_TILESET_BINDINGS.is_empty(),
            "an empty binding table would make every map resolve as unresolved, which is exactly \
             the silent-pass failure this table exists to prevent"
        );
        let mut previous: Option<&str> = None;
        for (map, tilesets) in COMBAT_TILESET_BINDINGS {
            assert_eq!(
                *map,
                map.to_ascii_lowercase(),
                "table keys must be lowercase: {map}"
            );
            assert!(map.ends_with(".smp"), "a binding key must be a .smp: {map}");
            if let Some(previous) = previous {
                assert!(
                    previous < *map,
                    "table must be strictly sorted for the binary search: {previous} then {map}"
                );
            }
            previous = Some(map);

            assert!(
                !tilesets.is_empty(),
                "{map} has an empty candidate list; a map with no candidates must be absent from \
                 the table, not present with nothing in it"
            );
            let mut sorted = tilesets.to_vec();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(
                sorted.as_slice(),
                *tilesets,
                "{map}'s candidates must be sorted and unique"
            );
            for tileset in *tilesets {
                assert_eq!(
                    *tileset,
                    tileset.to_ascii_lowercase(),
                    "candidate must be lowercase: {tileset}"
                );
                assert!(
                    tileset.ends_with(".til"),
                    "candidate must be a .til: {tileset}"
                );
            }
        }
    }

    /// Resolution is by **map name**, from the gamescript table -- never by map class.
    ///
    /// The four outcomes are all asserted on real table entries, and the pairings chosen are the
    /// ones that a class rule or a faith-name rule gets *wrong*: `aicave.smp` pairs with
    /// `aibldg01.til` and not `tilesa01.til`; `licave.smp` pairs with `libldg01.til` **and**
    /// `wabldg01.til`, neither of which a "li" prefix predicts on its own; `pathwoods.smp` really
    /// does pair with `tilesa01.til`, so the table is not merely "never tilesa01".
    #[test]
    fn combat_tilesets_resolve_by_map_name_from_the_gamescript_table() {
        use super::{TileSetResolution, WORLD_TILESET_MEMBER, resolve_tileset};
        use std::path::Path;

        // A single-valued binding, and one a class rule would get wrong.
        let resolution = resolve_tileset(Path::new("map/aicave.smp")).unwrap();
        assert_eq!(resolution.unique(), Some("aibldg01.til"));
        assert!(resolution.accepts("aibldg01.til"));
        assert!(!resolution.accepts("tilesa01.til"));
        // Case-insensitive on the map name, because `map/` is split .smp/.SMP.
        assert_eq!(
            resolve_tileset(Path::new("map/AICAVE.SMP")).unwrap().unique(),
            Some("aibldg01.til")
        );

        // A multi-valued binding: several encounters, several tilesets, no single answer.
        let resolution = resolve_tileset(Path::new("licave.smp")).unwrap();
        assert!(matches!(resolution, TileSetResolution::CombatAmbiguous(_)));
        assert_eq!(resolution.unique(), None);
        assert!(resolution.accepts("libldg01.til"));
        assert!(resolution.accepts("wabldg01.til"));
        assert!(!resolution.accepts("tilesa01.til"));
        assert!(resolution.describe().starts_with("ambiguous:"));

        // `tilesa01.til` is a legitimate answer for the few maps the scripts pair with it.
        assert_eq!(
            resolve_tileset(Path::new("pathwoods.smp")).unwrap().unique(),
            Some("tilesa01.til")
        );

        // A combat map with no binding: unresolved, with no candidates and no default.
        let resolution = resolve_tileset(Path::new("aibrks0.smp")).unwrap();
        assert_eq!(resolution, TileSetResolution::CombatUnresolved);
        assert_eq!(resolution.unique(), None);
        assert!(resolution.candidates().is_empty());
        assert!(!resolution.accepts("tilesa01.til"));
        assert!(!resolution.accepts("aibldg01.til"));
        assert_eq!(resolution.describe(), "unresolved");

        // World maps keep their single answer.
        let resolution = resolve_tileset(Path::new("URAK.scn")).unwrap();
        assert_eq!(resolution.unique(), Some(WORLD_TILESET_MEMBER));
        assert_eq!(WORLD_TILESET_MEMBER, "tilesb01.til");
        assert!(resolution.accepts("TILESB01.TIL"));

        // An unclassified extension has no answer at all.
        assert!(resolve_tileset(Path::new("notes.txt")).is_none());
    }

    /// A shipped tileset the scripts do not bind to this map is a mismatch; three other cases are
    /// deliberately not.
    ///
    /// **The regression this pins is the one that shipped.** The previous version refused
    /// `aicave.smp` + `aibldg01.til` -- the gamescript's own pairing -- and accepted
    /// `tilesa01.til`, whose slots a 64-slot atlas cannot show. Both directions are asserted here,
    /// so a return to a class rule fails rather than passing.
    #[test]
    fn a_mismatch_is_raised_only_against_a_recorded_binding() {
        use super::tileset_mismatch;
        use std::path::Path;

        // The gamescript's own pairing must NOT be a mismatch. This is the inverted refusal.
        assert!(
            tileset_mismatch(Path::new("map/aicave.smp"), Path::new("til/aibldg01.til")).is_none(),
            "the gamescript pairs aicave.smp with aibldg01.til; refusing it is the bug that shipped"
        );

        // And the tileset the old rule recommended must now be the mismatch.
        let mismatch =
            tileset_mismatch(Path::new("map/aicave.smp"), Path::new("til/tilesa01.til")).unwrap();
        assert_eq!(mismatch.supplied, "tilesa01.til");
        assert_eq!(mismatch.expected, ["aibldg01.til"]);
        assert!(mismatch.to_string().contains("aibldg01.til"));
        assert!(
            !mismatch.to_string().contains("Pass tilesa01.til instead"),
            "the old advice must not survive: {mismatch}"
        );

        // Either declared candidate of an ambiguous map is accepted.
        assert!(
            tileset_mismatch(Path::new("licave.smp"), Path::new("libldg01.til")).is_none()
        );
        assert!(
            tileset_mismatch(Path::new("licave.smp"), Path::new("wabldg01.til")).is_none()
        );
        // A tileset outside both the declared and the runtime-reachable sets is a mismatch, and
        // the advice names the **declared** set, not everything reachable.
        let mismatch = tileset_mismatch(Path::new("licave.smp"), Path::new("jeff01.til")).unwrap();
        assert_eq!(mismatch.expected, ["libldg01.til", "wabldg01.til"]);
        assert!(mismatch.to_string().contains("libldg01.til or wabldg01.til"));
        assert!(
            !mismatch.to_string().contains("cavelava.til"),
            "the suggestion must be the declared tileset, not a coarse reachable one: {mismatch}"
        );

        // An **unresolved** map cannot produce a mismatch: nothing is known, so nothing is refused.
        for tileset in ["tilesa01.til", "tilesb01.til", "aibldg01.til", "jeff01.til"] {
            assert!(
                tileset_mismatch(Path::new("aibrks0.smp"), Path::new(tileset)).is_none(),
                "{tileset} on an unbound map must pass: the table says nothing about it"
            );
        }

        // World maps still refuse a shipped combat tileset.
        let mismatch = tileset_mismatch(Path::new("URAK.scn"), Path::new("tilesa01.til")).unwrap();
        assert_eq!(mismatch.expected, ["tilesb01.til"]);

        // A modded tileset is presumed deliberate, for every class and every resolution state.
        for map in ["aicave.smp", "licave.smp", "aibrks0.smp", "URAK.scn"] {
            assert!(tileset_mismatch(Path::new(map), Path::new("mymod.til")).is_none());
        }

        // An unclassified extension has no rule to contradict.
        assert!(tileset_mismatch(Path::new("a.dat"), Path::new("tilesb01.til")).is_none());
    }

    /// Reporting is precise; refusing is permissive. Both halves are asserted.
    ///
    /// The plural selector form means an encounter may reach a tileset at runtime that is not the
    /// map's declared one. Refusing such a tileset would repeat the bug that shipped -- refusing
    /// the engine's own answer -- so `tileset_mismatch` tests against the union. But
    /// `resolve_tileset` must keep reporting only the declared binding, because the coarse set
    /// widens `demina.smp` from `debldg01.til` at 98.3% satisfaction to include `ruins01.til` at
    /// 81.3%, and presenting those as equals is what the separation exists to avoid.
    #[test]
    fn a_runtime_reachable_tileset_is_not_refused_but_is_not_reported_as_the_binding_either() {
        use super::{
            combat_tileset_array_candidates, paintable_tilesets, resolve_tileset, tileset_mismatch,
        };
        use std::path::Path;

        // `demina.smp` is declared `debldg01.til` and can reach three more at runtime.
        let reachable = combat_tileset_array_candidates("demina.smp");
        assert!(reachable.contains(&"ruins01.til"), "{reachable:?}");
        assert!(!reachable.contains(&"debldg01.til"), "a declared tileset is not 'extra'");

        // Reported answer: the declared one only.
        let resolution = resolve_tileset(Path::new("demina.smp")).unwrap();
        assert_eq!(resolution.unique(), Some("debldg01.til"));
        assert!(!resolution.accepts("ruins01.til"));

        // Refusal: permissive, so the reachable one passes.
        assert!(
            tileset_mismatch(Path::new("demina.smp"), Path::new("ruins01.til")).is_none(),
            "a tileset the engine may reach at runtime must not be refused"
        );
        assert!(
            tileset_mismatch(Path::new("demina.smp"), Path::new("debldg01.til")).is_none()
        );
        // Something in neither set is still refused, with the declared advice.
        let mismatch =
            tileset_mismatch(Path::new("demina.smp"), Path::new("tilesa01.til")).unwrap();
        assert_eq!(mismatch.expected, ["debldg01.til"]);

        // The union is what the gate uses, and it is sorted and duplicate-free.
        let paintable = paintable_tilesets(Path::new("demina.smp"));
        assert!(paintable.contains(&"debldg01.til"));
        assert!(paintable.contains(&"ruins01.til"));
        let mut sorted = paintable.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted, paintable);

        // A map with no array candidates has a union equal to its declared set.
        assert!(combat_tileset_array_candidates("pathwoods.smp").is_empty());
        assert_eq!(paintable_tilesets(Path::new("pathwoods.smp")), ["tilesa01.til"]);
        // And an unresolved map has an empty union, so nothing is refused for it.
        assert!(paintable_tilesets(Path::new("aibrks0.smp")).is_empty());
    }

    /// The array-candidate table's invariants, and that it is disjoint from the declared one.
    #[test]
    fn the_array_candidate_table_is_sorted_and_disjoint_from_the_declared_bindings() {
        use super::{COMBAT_TILESET_ARRAY_CANDIDATES, COMBAT_TILESET_BINDINGS};

        assert!(!COMBAT_TILESET_ARRAY_CANDIDATES.is_empty());
        let mut previous: Option<&str> = None;
        for (map, tilesets) in COMBAT_TILESET_ARRAY_CANDIDATES {
            assert_eq!(*map, map.to_ascii_lowercase());
            assert!(map.ends_with(".smp"));
            if let Some(previous) = previous {
                assert!(previous < *map, "must be sorted: {previous} then {map}");
            }
            previous = Some(map);
            assert!(!tilesets.is_empty(), "{map} has an empty extra-candidate list");
            let mut sorted = tilesets.to_vec();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(sorted.as_slice(), *tilesets, "{map}'s extras must be sorted and unique");

            // Disjoint from the declared set, or the "extra" framing is a lie and the two tables
            // would double-count.
            if let Ok(index) =
                COMBAT_TILESET_BINDINGS.binary_search_by(|(name, _)| name.cmp(map))
            {
                for tileset in *tilesets {
                    assert!(
                        !COMBAT_TILESET_BINDINGS[index].1.contains(tileset),
                        "{map}: {tileset} is both declared and 'extra'"
                    );
                }
            }
        }
    }

    /// `combattileset` is recorded, and is deliberately **not** the resolver for `.smp`.
    ///
    /// Measured and not in doubt: it occurs exactly once in `gs.mpq`. What was wrong was the
    /// inference. This test pins the distinction so the constant cannot quietly become the
    /// fallback again.
    #[test]
    fn the_generated_combat_tileset_is_not_what_any_shipped_smp_resolves_to_by_default() {
        use super::{
            COMBAT_TILESET_BINDINGS, GENERATED_COMBAT_TILESET_MEMBER, TileSetResolution,
            resolve_tileset,
        };
        use std::path::Path;

        assert_eq!(GENERATED_COMBAT_TILESET_MEMBER, "tilesa01.til");

        // Exactly three table entries name it, and they are pairings, not a default.
        let naming: Vec<&str> = COMBAT_TILESET_BINDINGS
            .iter()
            .filter(|(_, tilesets)| {
                tilesets.contains(&GENERATED_COMBAT_TILESET_MEMBER)
            })
            .map(|(map, _)| *map)
            .collect();
        assert!(
            naming.contains(&"pathwoods.smp"),
            "pathwoods.smp is the control: the scripts really do pair it with tilesa01.til"
        );
        assert!(
            naming.len() < COMBAT_TILESET_BINDINGS.len() / 10,
            "tilesa01.til is a rare pairing, not the rule; it names {} of {} entries",
            naming.len(),
            COMBAT_TILESET_BINDINGS.len()
        );

        // An unbound map does not fall back to it.
        assert_eq!(
            resolve_tileset(Path::new("aibrks0.smp")).unwrap(),
            TileSetResolution::CombatUnresolved
        );
    }

    /// All 26 shipped names, and the three that the assignment turns on.
    #[test]
    fn the_shipped_tileset_names_are_recognised_case_insensitively() {
        use super::{SHIPPED_TILESET_MEMBERS, is_shipped_tileset};

        assert_eq!(SHIPPED_TILESET_MEMBERS.len(), 26);
        // Sorted and unique, so a later edit cannot quietly duplicate or drop one.
        let mut sorted = SHIPPED_TILESET_MEMBERS.to_vec();
        sorted.sort_unstable();
        assert_eq!(sorted.as_slice(), SHIPPED_TILESET_MEMBERS.as_slice());
        sorted.dedup();
        assert_eq!(sorted.len(), 26);

        assert!(is_shipped_tileset("tilesa01.til"));
        assert!(is_shipped_tileset("TilesB01.TIL"));
        assert!(is_shipped_tileset("aibldg01.til"));
        assert!(!is_shipped_tileset("mymod.til"));
        assert!(!is_shipped_tileset("tilesa01"));
    }

    /// A tileset deliberately **unlike** the shipped corpus in every way the writer could get wrong.
    ///
    /// The 26 shipped files are uniformly CRLF, uniformly end with a newline, quote every
    /// description, write every row with all eleven fields, and never repeat a key. A fixture that
    /// copied those habits could not fail on any of them. So this one mixes CRLF and bare LF, ends
    /// without a terminator, leaves one description unquoted, pads with tabs in one row and spaces
    /// in another, carries a trailing comment on a record line, an unknown key, and a `TILE=` row
    /// that stops early. Tile 2 is deliberately the **only** tile of terrain type 1, so the
    /// last-tile-of-a-terrain refusal has something to fire on.
    fn awkward_tileset() -> Vec<u8> {
        let mut source = String::new();
        source.push_str("LBM=awkward.lbm\r\n");
        source.push_str("TILES= 4, 3 \r\n");
        source.push_str("TILESIZE= 32, 32\r\n");
        source.push_str("; a=flags\r\n");
        source.push_str("\r\n");
        source.push_str("TERRAINTYPE= 0, 137, \"intermediate\",\t0,\t0,\t9999,\t4, 1, 1, 2, 2\r\n");
        source.push_str("TERRAINTYPE= 1, 112, bare water   ,      1,      0,      1,   4, 15, 1, 2, 2\r\n");
        source.push_str("PALETTE= not a key this writer models\r\n");
        source.push_str("TILE=      0, 0,  *,  *,  *,  *,  *,  *,  *,  *,   0 ; the empty slot\n");
        source.push_str("TILE=      1, 0,  ~1,  *,  1,  *,  ~1,  *,  1,  *,   1\r\n");
        source.push_str("TILE=      2, 1,  1,  1,  1,  1,  1,  1,  1,  1,   2\r\n");
        source.push_str("TILE=      5, 0\r\n");
        source.push_str("TILE=      6, 0,  *,  *,  *,  *,  *,  *,  *,  *,   6");
        source.into_bytes()
    }

    /// Mixed terminators, a missing final newline and every oddity above survive a read and write.
    ///
    /// This is the weak half of the round-trip claim and is labelled as such on
    /// [`TileSetDocument`]: the lines are carried, so what it tests is the line splitter and the
    /// terminators, not the field model. It earns its place because it is exactly what the splitter
    /// gets wrong -- a normalised `\n`, or a final line silently given a terminator it never had.
    #[test]
    fn an_unedited_document_re_encodes_byte_for_byte() {
        let source = awkward_tileset();
        let document = TileSetDocument::parse(&source).unwrap();

        assert_eq!(document.to_bytes(), source);
        assert_eq!(document.definition().atlas_member, "awkward.lbm");
        assert_eq!(document.definition().tiles.len(), 5);
    }

    /// The audit is **not** an identity, and this is the test that proves it.
    ///
    /// Every field here parses to the same value the corpus's spelling would, and every one is
    /// spelled differently: `1|1` collapses to a one-element set, `1|0` is out of ascending order,
    /// and `007` has leading zeros. An audit that compared the parser's answer to itself, or that
    /// rebuilt from the retained text rather than from the value, would report these as rebuilt.
    #[test]
    fn the_field_audit_names_columns_whose_spelling_the_value_model_loses() {
        let source = b"LBM=a.lbm\r\nTILES= 4, 1\r\nTILESIZE= 32, 32\r\n\
TERRAINTYPE= 0, 007, \"a\", 0, 0, 9999, 4, 1, 1, 2, 2\r\n\
TERRAINTYPE= 1, 112, \"b\", 0, 0, 9999, 4, 1, 1, 2, 2\r\n\
TILE= 0, 0, 1|1, 1|0, *, *, *, *, *, *, 0\r\n"
            .to_vec();
        let audit = TileSetDocument::parse(&source).unwrap().field_rebuild_audit();

        assert_eq!(audit.values_checked - audit.values_rebuilt, 3);
        assert_eq!(audit.mismatches.len(), 3);
        let joined = audit.mismatches.join(" | ");
        assert!(joined.contains("file `007`, rebuilt `7`"), "{joined}");
        assert!(joined.contains("file `1|1`, rebuilt `1`"), "{joined}");
        assert!(joined.contains("file `1|0`, rebuilt `0|1`"), "{joined}");
    }

    /// An edit replaces one field's characters and leaves every other byte of the file alone.
    ///
    /// Asserted line by line rather than on the one line that changed: a writer that re-rendered
    /// the row it touched would pass a check that only looked at the value, while quietly
    /// normalising the tabs of a row nobody named.
    #[test]
    fn an_edit_changes_one_field_and_leaves_every_other_line_alone() {
        let source = awkward_tileset();
        let mut document = TileSetDocument::parse(&source).unwrap();

        document
            .set_tile_neighbour(1, Direction::East, &NeighbourConstraint::Any)
            .unwrap();
        let written = document.to_bytes();

        let before = String::from_utf8(source).unwrap();
        let after = String::from_utf8(written).unwrap();
        let changed: Vec<(&str, &str)> = before
            .split_inclusive('\n')
            .zip(after.split_inclusive('\n'))
            .filter(|(before, after)| before != after)
            .collect();
        assert_eq!(changed.len(), 1, "{changed:?}");
        assert_eq!(
            changed[0].0,
            "TILE=      1, 0,  ~1,  *,  1,  *,  ~1,  *,  1,  *,   1\r\n"
        );
        assert_eq!(
            changed[0].1,
            "TILE=      1, 0,  ~1,  *,  *,  *,  ~1,  *,  1,  *,   1\r\n"
        );
        assert_eq!(
            *document.definition().tiles[&1].neighbour(Direction::East),
            NeighbourConstraint::Any
        );
    }

    /// Setting a field to the value it already holds is a no-op at the byte level.
    ///
    /// The calibration that has to run before any diff means anything: if the do-nothing case
    /// already rewrites tabs or re-quotes a description, every later edit reads as noise. Both
    /// descriptions here are covered, because the quoted and the bare one take different paths.
    #[test]
    fn a_no_op_edit_reproduces_the_file_exactly() {
        let source = awkward_tileset();
        let mut document = TileSetDocument::parse(&source).unwrap();

        document.set_atlas_member("awkward.lbm").unwrap();
        document.set_grid(4, 3).unwrap();
        document.set_tile_terrain_type(1, 0).unwrap();
        document
            .set_tile_neighbour(1, Direction::North, &NeighbourConstraint::NoneOf([1].into()))
            .unwrap();
        document
            .set_terrain_field(0, TerrainColumn::Description, "intermediate")
            .unwrap();
        document
            .set_terrain_field(1, TerrainColumn::Description, "bare water")
            .unwrap();
        document
            .set_terrain_field(1, TerrainColumn::MovementCost, "1")
            .unwrap();

        assert_eq!(document.to_bytes(), source);
    }

    /// A description that was written without quotes stays without them, and one that had them
    /// keeps them. The file's quoting is the file's.
    #[test]
    fn a_descriptions_quoting_is_carried_rather_than_imposed() {
        let mut document = TileSetDocument::parse(&awkward_tileset()).unwrap();

        document
            .set_terrain_field(0, TerrainColumn::Description, "quoted still")
            .unwrap();
        document
            .set_terrain_field(1, TerrainColumn::Description, "bare still")
            .unwrap();
        let written = String::from_utf8(document.to_bytes()).unwrap();

        assert!(written.contains("\"quoted still\","), "{written}");
        assert!(written.contains(" bare still   ,"), "{written}");
        assert_eq!(document.definition().terrain_types[&1].description, "bare still");
    }

    /// Where a key is written twice the **last** one decides, because that is what the value
    /// parser does. Editing the first would change a line the parser ignores and report success.
    #[test]
    fn editing_a_repeated_key_hits_the_line_the_parser_honours() {
        let source = b"LBM=first.lbm\r\nLBM=second.lbm\r\nTILES= 2, 1\r\nTILESIZE= 32, 32\r\n\
TERRAINTYPE= 0, 137, \"a\", 0, 0, 9999, 4, 1, 1, 2, 2\r\n\
TILE= 0, 0, *, *, *, *, *, *, *, *, 0\r\n"
            .to_vec();
        let mut document = TileSetDocument::parse(&source).unwrap();
        assert_eq!(document.definition().atlas_member, "second.lbm");

        document.set_atlas_member("third.lbm").unwrap();
        let written = String::from_utf8(document.to_bytes()).unwrap();

        assert!(written.contains("LBM=first.lbm"), "{written}");
        assert!(written.contains("LBM=third.lbm"), "{written}");
        assert_eq!(document.definition().atlas_member, "third.lbm");
    }

    /// Every refusal, each asserted on its **reason** rather than on `is_err`.
    ///
    /// A bare `is_err` would pass if every one of these failed for the same wrong cause -- a
    /// mis-parsed fixture, say -- which is how a refusal suite ends up proving only that the
    /// function returns errors.
    #[test]
    fn refusals_name_what_they_refuse_and_why() {
        let source = awkward_tileset();
        let mut document = TileSetDocument::parse(&source).unwrap();

        let refusal = |result: Result<(), TileError>| result.unwrap_err().to_string();

        // A record that is not in the file is never minted.
        let message = refusal(document.set_tile_terrain_type(3, 0));
        assert!(message.contains("no TILE= row for slot 3"), "{message}");
        assert!(message.contains("does not mint one"), "{message}");
        let message = refusal(document.set_terrain_field(9, TerrainColumn::PaletteColor, "1"));
        assert!(message.contains("no TERRAINTYPE= row for 9"), "{message}");

        // A tile pointing at a terrain type the file never declares can never be selected.
        let message = refusal(document.set_tile_terrain_type(0, 7));
        assert!(message.contains("no TERRAINTYPE= row for 7"), "{message}");

        // Tile 2 is the only tile of terrain 1; moving it would make every cell holding it
        // unreadable.
        let message = refusal(document.set_tile_terrain_type(2, 0));
        assert!(message.contains("only tile of terrain type 1"), "{message}");

        // Tile 5's row stops after two fields, so it can never be painted; writing one column
        // would make it look complete.
        let message = refusal(document.set_tile_neighbour(5, Direction::North, &NeighbourConstraint::Any));
        assert!(message.contains("did not declare all eight"), "{message}");
        assert!(message.contains("column n is missing"), "{message}");

        // A constraint may only name terrain types the file declares.
        let message = refusal(document.set_tile_neighbour(
            1,
            Direction::North,
            &NeighbourConstraint::OneOf([0, 4, 9].into()),
        ));
        assert!(message.contains("terrain type(s) 4, 9"), "{message}");

        // The four columns the shipped headers disagree about.
        for (column, expected) in [
            ('d', "call column d food"),
            ('e', "call column e ore"),
            ('g', "call this column unused"),
            ('h', "call this column unused"),
        ] {
            let message = refusal(document.set_terrain_field(0, TerrainColumn::Unnamed(column), "3"));
            assert!(message.contains(expected), "{column}: {message}");
            assert!(message.contains("minting a field"), "{column}: {message}");
        }

        // Columns whose numeric vocabulary is bounded by what the corpus declares, asserted from
        // both sides: 2 is what tilesb01.til's header declares and is accepted, 3 is what neither
        // shipped header declares. A bound tested only from the refusing side cannot fail on being
        // in the wrong place.
        {
            let mut accepted = TileSetDocument::parse(&source).unwrap();
            accepted
                .set_terrain_field(0, TerrainColumn::Passability, "2")
                .unwrap();
            assert_eq!(
                accepted.definition().terrain_types[&0].passability,
                Some(Passability::Impassable)
            );
        }
        let message = refusal(document.set_terrain_field(0, TerrainColumn::Passability, "3"));
        assert!(message.contains("outside the vocabulary"), "{message}");
        let message = refusal(document.set_terrain_field(0, TerrainColumn::PaletteColor, "256"));
        assert!(message.contains("past 255"), "{message}");
        let message = refusal(document.set_terrain_field(0, TerrainColumn::MinElevation, "10000"));
        assert!(message.contains("10000..9999"), "{message}");
        assert!(message.contains("inverted"), "{message}");

        // A description that would change how the line parses.
        let message = refusal(document.set_terrain_field(0, TerrainColumn::Description, "a,b"));
        assert!(message.contains("would change how the TERRAINTYPE= line parses"), "{message}");
        let message = refusal(document.set_terrain_field(0, TerrainColumn::Description, "  "));
        assert!(message.contains("leave the terrain type unnamed"), "{message}");

        // The atlas name.
        let message = refusal(document.set_atlas_member("custom.png"));
        assert!(message.contains("is not a .lbm"), "{message}");
        let message = refusal(document.set_atlas_member("two words.lbm"));
        assert!(message.contains("would change how the LBM= line parses"), "{message}");
        let message = refusal(document.set_atlas_member(""));
        assert!(message.contains("no image"), "{message}");

        // Re-columning repaints every declared tile with a different picture.
        let message = refusal(document.set_grid(2, 6));
        assert!(message.contains("moves every one of the 5 declared tiles"), "{message}");
        assert!(message.contains("Changing rows alone is safe"), "{message}");

        // Shrinking past a declared tile names the tiles that would be orphaned.
        let message = refusal(document.set_grid(4, 1));
        assert!(message.contains("leaves 2 declared tile(s) outside it: 5, 6"), "{message}");

        // The two columns whose meaning nothing sources at all.
        let message = refusal(document.set_tile_size(64, 64));
        assert!(message.contains("all 26 shipped tilesets declare 32x32"), "{message}");
        let message = refusal(document.set_tile_pattern_index(1, 4));
        assert!(message.contains("the column's rule is not known"), "{message}");

        // Nothing above changed a byte.
        assert_eq!(document.to_bytes(), source);
    }

    /// A row shorter than the column an edit names is refused rather than extended.
    ///
    /// Extending it would mean inventing values for every column in between, which is minting by
    /// another route. No shipped row is short, so this is the modded-file case.
    #[test]
    fn a_column_the_row_stops_before_is_refused_rather_than_appended() {
        let source = b"LBM=a.lbm\r\nTILES= 2, 1\r\nTILESIZE= 32, 32\r\n\
TERRAINTYPE= 0, 137, \"a\"\r\n\
TILE= 0, 0, *, *, *, *, *, *, *, *, 0\r\n"
            .to_vec();
        let mut document = TileSetDocument::parse(&source).unwrap();

        let message = document
            .set_terrain_field(0, TerrainColumn::MovementCost, "3")
            .unwrap_err()
            .to_string();

        assert!(message.contains("stops after 3 field(s)"), "{message}");
        assert!(message.contains("will not extend a row"), "{message}");
        assert_eq!(document.to_bytes(), source);
    }

    /// Both sides of the two guards, because a bound tested from one side only cannot fail on
    /// being in the wrong place.
    #[test]
    fn the_capacity_and_palette_guards_hold_at_their_edges() {
        let source = b"LBM=a.lbm\r\nTILES= 16, 64\r\nTILESIZE= 32, 32\r\n\
TERRAINTYPE= 0, 137, \"a\", 0, 0, 9999, 4, 1, 1, 2, 2\r\n\
TILE= 0, 0, *, *, *, *, *, *, *, *, 0\r\n"
            .to_vec();

        // 16 x 64 is exactly MAX_ATLAS_CAPACITY and is accepted; one row more is not.
        let mut document = TileSetDocument::parse(&source).unwrap();
        assert_eq!(document.definition().atlas_capacity(), MAX_ATLAS_CAPACITY);
        document.set_grid(16, 64).unwrap();
        let message = document.set_grid(16, 65).unwrap_err().to_string();
        assert!(message.contains("1040 slots"), "{message}");
        assert!(message.contains("past the 1024"), "{message}");

        // The palette index at its edge and one past it.
        document
            .set_terrain_field(0, TerrainColumn::PaletteColor, &MAX_PALETTE_INDEX.to_string())
            .unwrap();
        let message = document
            .set_terrain_field(0, TerrainColumn::PaletteColor, &(MAX_PALETTE_INDEX + 1).to_string())
            .unwrap_err()
            .to_string();
        assert!(message.contains("past 255"), "{message}");
    }

    /// A column name on a command line resolves to the column it names, in both directions.
    #[test]
    fn column_names_resolve_to_the_columns_they_name() {
        assert_eq!(
            TerrainColumn::parse("movement-cost"),
            Some(TerrainColumn::MovementCost)
        );
        assert_eq!(TerrainColumn::parse("F"), Some(TerrainColumn::MovementCost));
        assert_eq!(TerrainColumn::parse("d"), Some(TerrainColumn::Unnamed('d')));
        assert_eq!(TerrainColumn::parse("food"), None);
        for column in TerrainColumn::ALL {
            assert_eq!(TerrainColumn::parse(&column.name()), Some(column));
        }
        for direction in Direction::ALL {
            assert_eq!(
                Direction::from_column_name(direction.column_name()),
                Some(direction)
            );
            assert_eq!(
                Direction::from_column_name(&direction.column_name().to_uppercase()),
                Some(direction)
            );
        }
        assert_eq!(Direction::from_column_name("north"), None);
    }

    /// Every constraint form a shipped column uses survives a parse and a re-spelling.
    #[test]
    fn a_neighbour_column_round_trips_through_its_own_syntax() {
        for column in ["*", "6", "~6", "6|9", "~6|9", "0|1|2"] {
            let constraint = NeighbourConstraint::parse_column(column).unwrap();
            assert_eq!(constraint.to_column(), column);
        }
        assert!(NeighbourConstraint::parse_column("").is_err());
        assert!(NeighbourConstraint::parse_column("~").is_err());
        assert!(NeighbourConstraint::parse_column("six").is_err());
    }

    /// Passability survives the trip through the enum, including a value neither header names.
    #[test]
    fn passability_keeps_a_value_no_shipped_header_declares() {
        for value in [0, 1, 2, 7, 4242] {
            assert_eq!(Passability::from_value(value).value(), value);
        }
    }


    /// A bare-CR file is refused, and refused with a message that names the line endings.
    ///
    /// Two separate claims. **Refusing is correct** and is not a gap: no shipped `.til` uses bare
    /// CR, and nothing says the engine would read one, so accepting it would invent a capability.
    /// But the refusal it used to earn was "tile definition has no TILES dimensions" on a file
    /// whose second record *is* `TILES=`, which sends the reader to the wrong line. The control is
    /// the same content with CRLF, which must still parse.
    #[test]
    fn a_bare_cr_file_is_refused_for_its_line_endings_and_not_for_a_missing_key() {
        let records = [
            "LBM=a.lbm",
            "TILES= 2, 1",
            "TILESIZE= 32, 32",
            "TERRAINTYPE= 0, 137, \"a\", 0, 0, 9999, 4, 1, 1, 2, 2",
            "TILE= 0, 0, *, *, *, *, *, *, *, *, 0",
        ];

        let control = TileSetDefinition::parse(format!("{}\r\n", records.join("\r\n")).as_bytes());
        assert!(control.is_ok(), "{control:?}");

        let message = TileSetDefinition::parse(format!("{}\r", records.join("\r")).as_bytes())
            .unwrap_err()
            .to_string();
        assert!(message.contains("bare CR"), "{message}");
        assert!(message.contains("5 record(s)"), "{message}");
        assert!(!message.contains("no TILES dimensions"), "{message}");
    }

    /// A row with one unreadable neighbour column has its **other seven** audited.
    ///
    /// The audit used to skip all eight whenever `constraints_declared` was false, so a file could
    /// carry a malformed column and be reported with zero failures and eight fewer values checked.
    /// A denominator that silently shrinks on bad input undercuts `values-rebuilt`, which is the
    /// one number in the sweep that is supposed to be able to fail.
    #[test]
    fn a_row_with_one_unreadable_column_still_audits_the_other_seven() {
        let good = b"LBM=a.lbm\r\nTILES= 2, 1\r\nTILESIZE= 32, 32\r\n\
TERRAINTYPE= 0, 137, \"a\", 0, 0, 9999, 4, 1, 1, 2, 2\r\n\
TILE= 0, 0, *, *, *, *, *, *, *, *, 0\r\n"
            .to_vec();
        let bad = b"LBM=a.lbm\r\nTILES= 2, 1\r\nTILESIZE= 32, 32\r\n\
TERRAINTYPE= 0, 137, \"a\", 0, 0, 9999, 4, 1, 1, 2, 2\r\n\
TILE= 0, 0, *, *, bogus, *, *, *, *, *, 0\r\n"
            .to_vec();

        let good = TileSetDocument::parse(&good).unwrap().field_rebuild_audit();
        let bad = TileSetDocument::parse(&bad).unwrap().field_rebuild_audit();

        // The malformed row is audited just as widely as the good one: same denominator.
        assert_eq!(bad.values_checked, good.values_checked);
        assert_eq!(good.values_checked - good.values_rebuilt, 0);
        // And the one bad column is the one finding, named by its column.
        assert_eq!(bad.mismatches.len(), 1, "{:?}", bad.mismatches);
        assert!(
            bad.mismatches[0].contains("column e: file `bogus`, rebuilt `*`"),
            "{:?}",
            bad.mismatches
        );
    }

    /// A row of commas is refused rather than allocated.
    ///
    /// Both sides of [`MAX_RECORD_FIELDS`], because a bound tested only from the refusing side
    /// cannot fail on being in the wrong place. The hostile case is the reason: the field count
    /// comes from the file, and without a bound a row of ten million commas is ten million
    /// allocations before anything notices.
    #[test]
    fn a_row_carrying_more_fields_than_any_record_has_is_refused() {
        let header = "LBM=a.lbm\r\nTILES= 2, 1\r\nTILESIZE= 32, 32\r\n\
TERRAINTYPE= 0, 137, \"a\", 0, 0, 9999, 4, 1, 1, 2, 2\r\n";
        let row = |fields: usize| {
            let mut columns = vec!["0".to_owned(), "0".to_owned()];
            columns.resize(fields, "*".to_owned());
            format!("{header}TILE= {}\r\n", columns.join(", "))
        };

        // At the bound, and one past it.
        let accepted = TileSetDefinition::parse(row(MAX_RECORD_FIELDS).as_bytes());
        assert!(accepted.is_ok(), "{accepted:?}");
        let message = TileSetDefinition::parse(row(MAX_RECORD_FIELDS + 1).as_bytes())
            .unwrap_err()
            .to_string();
        assert!(message.contains("more than 64 fields"), "{message}");

        // The hostile input the bound exists for returns rather than allocating per comma.
        let message = TileSetDefinition::parse(row(200_000).as_bytes())
            .unwrap_err()
            .to_string();
        assert!(message.contains("more than 64 fields"), "{message}");
    }

    /// Replacing the rightmost field first is load-bearing, not tidiness.
    ///
    /// `set_grid` rewrites two spans on one line. Doing the **left** one first shortens the line
    /// under the right one's span, which then points past the end of the string -- a panic, not a
    /// wrong answer. Two inputs reach it: a file with no `TILE=` rows at all, and a file whose
    /// grid is spelled wider than its value needs (`016`). Both are here because the second is the
    /// one a hand-written corpus actually contains.
    #[test]
    fn setting_the_grid_replaces_the_rightmost_field_first() {
        let tileless = b"LBM=a.lbm\r\nTILES= 16, 4\r\nTILESIZE= 32, 32\r\n\
TERRAINTYPE= 0, 137, \"a\", 0, 0, 9999, 4, 1, 1, 2, 2\r\n"
            .to_vec();
        let mut document = TileSetDocument::parse(&tileless).unwrap();
        document.set_grid(4, 5).unwrap();
        assert_eq!(
            String::from_utf8(document.to_bytes()).unwrap().lines().nth(1),
            Some("TILES= 4, 5")
        );
        assert_eq!(document.definition().columns, 4);
        assert_eq!(document.definition().rows, 5);

        let padded = b"LBM=a.lbm\r\nTILES= 016, 8\r\nTILESIZE= 32, 32\r\n\
TERRAINTYPE= 0, 137, \"a\", 0, 0, 9999, 4, 1, 1, 2, 2\r\n\
TILE= 0, 0, *, *, *, *, *, *, *, *, 0\r\n"
            .to_vec();
        let mut document = TileSetDocument::parse(&padded).unwrap();
        assert_eq!(document.definition().columns, 16);
        document.set_grid(16, 9).unwrap();
        assert_eq!(
            String::from_utf8(document.to_bytes()).unwrap().lines().nth(1),
            Some("TILES= 16, 9")
        );
        assert_eq!(document.definition().rows, 9);
    }

    /// The edit is verified **through the parser**, and a change that does not read back is
    /// refused with the original line restored.
    ///
    /// This is the headline property, and until now nothing failed when the check was removed.
    /// `"  padded  "` is the input that reaches it: it passes every guard, is written into the
    /// line, and comes back from the reader as `padded` -- a different value from the one asked
    /// for. Without the check the caller is told the edit succeeded and the file says something
    /// else.
    ///
    /// The second half is the restore. After the refusal the bytes must be the original ones
    /// **and the line's field spans must have been re-derived from them** -- a stale span set left
    /// over from the rejected text would splice the next edit at the wrong offset, which the
    /// follow-up edit here would show as a mangled line rather than an error.
    #[test]
    fn an_edit_that_does_not_read_back_is_refused_and_the_line_is_restored() {
        let source = awkward_tileset();
        let mut document = TileSetDocument::parse(&source).unwrap();

        let message = document
            .set_terrain_field(0, TerrainColumn::Description, "  padded  ")
            .unwrap_err()
            .to_string();

        assert!(message.contains("did not read back"), "{message}");
        assert!(message.contains("trimmed"), "{message}");
        assert_eq!(document.to_bytes(), source);
        assert_eq!(document.definition().terrain_types[&0].description, "intermediate");

        // The spans survived the restore: this lands exactly where the refused one would have.
        document
            .set_terrain_field(0, TerrainColumn::Description, "renamed")
            .unwrap();
        let written = String::from_utf8(document.to_bytes()).unwrap();
        assert!(
            written.contains("TERRAINTYPE= 0, 137, \"renamed\",\t0,\t0,\t9999,\t4, 1, 1, 2, 2"),
            "{written}"
        );
    }

    /// The refusals the first suite left one-sided, each on its own.
    ///
    /// Every one of these was removable with the suite still green: the maximum-elevation side of
    /// the inversion check, the `;` and `"` arms of the description guard, the unreadable-but-
    /// full-width row, and `TILESIZE=` at the value it already holds. A refusal that is never
    /// removed is a refusal that is never tested.
    #[test]
    fn the_one_sided_refusals_hold_from_their_other_side_too() {
        let source = awkward_tileset();
        let mut document = TileSetDocument::parse(&source).unwrap();

        // Inversion, driven from the maximum rather than the minimum. Terrain 1's range is 0..1,
        // so the minimum is first raised to meet the maximum -- on its own document, since that
        // part is a real edit and the pristine one is asserted unchanged at the end.
        let mut elevations = TileSetDocument::parse(&source).unwrap();
        elevations
            .set_terrain_field(1, TerrainColumn::MinElevation, "1")
            .unwrap();
        let message = elevations
            .set_terrain_field(1, TerrainColumn::MaxElevation, "0")
            .unwrap_err()
            .to_string();
        assert!(message.contains("1..0"), "{message}");
        assert!(message.contains("inverted"), "{message}");

        // The description guard's other two characters, each alone.
        for (description, character) in [("a;b", ';'), ("a\"b", '"')] {
            let message = document
                .set_terrain_field(0, TerrainColumn::Description, description)
                .unwrap_err()
                .to_string();
            assert!(message.contains(&format!("{character:?}")), "{message}");
            assert!(
                message.contains("would change how the TERRAINTYPE= line parses"),
                "{message}"
            );
        }

        // A row that is full width and still unreadable: eleven fields, one of them nonsense. The
        // truncated-row case is covered elsewhere and reaches the same refusal by a different
        // route, which is why both are needed.
        let unreadable = b"LBM=a.lbm\r\nTILES= 2, 1\r\nTILESIZE= 32, 32\r\n\
TERRAINTYPE= 0, 137, \"a\", 0, 0, 9999, 4, 1, 1, 2, 2\r\n\
TILE= 0, 0, *, *, *, bogus, *, *, *, *, 0\r\n"
            .to_vec();
        let mut full_width = TileSetDocument::parse(&unreadable).unwrap();
        assert_eq!(full_width.definition().tiles[&0].neighbours.len(), 8);
        let message = full_width
            .set_tile_neighbour(0, Direction::North, &NeighbourConstraint::Any)
            .unwrap_err()
            .to_string();
        assert!(message.contains("did not declare all eight"), "{message}");
        assert!(message.contains("column se is missing or unreadable"), "{message}");
        assert_eq!(full_width.to_bytes(), unreadable);

        // TILESIZE= is refused at the value it already holds, not merely at a new one: the reason
        // is that nothing sources the field, and that does not depend on what is being written.
        let message = document.set_tile_size(32, 32).unwrap_err().to_string();
        assert!(message.contains("refusing to declare a 32x32 tile"), "{message}");
        assert!(message.contains("all 26 shipped tilesets declare 32x32"), "{message}");

        // An atlas name with nothing before its extension.
        let message = document.set_atlas_member(".lbm").unwrap_err().to_string();
        assert!(message.contains("no name before its extension"), "{message}");

        assert_eq!(document.to_bytes(), source);
    }

}
