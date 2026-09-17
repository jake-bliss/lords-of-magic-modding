use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

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

    fn parse(field: &str, line: usize) -> Result<Self, TileError> {
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
                "empty neighbour constraint on line {line}"
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
    pub fn parse(source: &[u8]) -> Result<Self, TileError> {
        let text = std::str::from_utf8(source)
            .map_err(|error| TileError::new(format!("tile definition is not UTF-8: {error}")))?;
        let mut atlas_member = None;
        let mut dimensions = None;
        let mut tile_size = None;
        let mut terrain_types = BTreeMap::new();
        let mut tiles = BTreeMap::new();

        for (line_index, original_line) in text.lines().enumerate() {
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
                "TILES" => dimensions = Some(parse_pair(value, line_number, "TILES")?),
                "TILESIZE" => tile_size = Some(parse_pair(value, line_number, "TILESIZE")?),
                "TERRAINTYPE" => {
                    let fields = csv_fields(value, line_number)?;
                    if fields.len() < 3 {
                        return Err(TileError::new(format!(
                            "TERRAINTYPE on line {line_number} has fewer than three fields"
                        )));
                    }
                    let index = parse_u32(&fields[0], line_number, "terrain type index")?;
                    let palette_color =
                        parse_u32(&fields[1], line_number, "terrain palette color")?;
                    // Columns a..h follow the description. Only the four the file's own header
                    // names are given a name here; d, e, g and h are kept as text because the two
                    // shipped tilesets disagree about what d and e mean.
                    // Unreadable trailing columns are recorded as absent, not rejected, for the
                    // same reason as the tile rows: the renderer does not read them.
                    let trailing = |position: usize, name: &str| -> Option<u32> {
                        fields
                            .get(position)
                            .filter(|field| !field.is_empty())
                            .and_then(|field| parse_u32(field, line_number, name).ok())
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
                    let fields = csv_fields(value, line_number)?;
                    if fields.len() < 2 {
                        return Err(TileError::new(format!(
                            "TILE on line {line_number} has fewer than two fields"
                        )));
                    }
                    let index = parse_u32(&fields[0], line_number, "tile index")?;
                    let terrain_type = parse_u32(&fields[1], line_number, "tile terrain type")?;
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
                            .map(|field| NeighbourConstraint::parse(field, line_number))
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
                        .and_then(|field| parse_u32(field, line_number, "tile pattern index").ok());
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

/// The `.til` member the shipped engine reads this class of map through.
///
/// **Observed in a local binary, 2026-09-17, and this closes "which tileset each `.smp` uses".**
/// The engine does not derive a combat map's tileset from the file, from its name, or from its
/// terrain: it sets one globally at startup and never changes it. `START.GS` lines 74-75 of the
/// GS5R3 `gs.mpq` read
///
/// ```text
/// "til/tilesb01.til"dup /currenttileset exch def maptileset
/// "til/tilesa01.til"combattileset
/// ```
///
/// and `combattileset` appears **exactly once in all 1,696 gamescript members** -- it is never
/// re-set, never parameterised, and takes no map argument. So every one of the 337 `.smp` files is
/// read through `tilesa01.til` and every `.scn`, `.lgd` and `.map` through `tilesb01.til`.
///
/// The other 24 `.til` members are the **terrain editor's** interactive tile palette: `terredit`'s
/// `tileselector` feeds whatever the user picks to `maptileset`, and nothing in the corpus feeds
/// any of them to `combattileset`. A per-faith or per-location rule read off the `.smp` filenames
/// -- `AIBLDG01.SMP` against `aibldg01.til` -- fits 236 of the 263 faith-prefixed maps and is
/// nonetheless **wrong**; see `docs/map-format.md`.
pub const fn engine_tileset_member(class: MapClass) -> &'static str {
    match class {
        MapClass::World => "tilesb01.til",
        MapClass::Combat => "tilesa01.til",
    }
}

/// The 26 `.til` members shipped in GS5R3 `pic.mpq`, lowercase, sorted.
///
/// Only the **names** are recorded; no tileset is committed. This exists so a supplied tileset can
/// be told apart from a modded one: a caller who passes a shipped tileset the engine does not use
/// for that map class has made a mistake worth refusing, while a caller who passes `mymod.til`
/// has not, and must keep working.
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

/// A supplied tileset that the engine demonstrably does not use for this map class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TileSetMismatch {
    pub class: MapClass,
    pub supplied: String,
    pub expected: &'static str,
}

impl fmt::Display for TileSetMismatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} is a shipped tileset, but the engine reads a {} through {}: START.GS sets \
             {} and it is never re-set. Pass {} instead, or a tileset of your own -- a \
             modded name is accepted as-is",
            self.supplied,
            self.class.description(),
            self.expected,
            match self.class {
                MapClass::Combat => "combattileset",
                MapClass::World => "maptileset",
            },
            self.expected,
        )
    }
}

/// Whether a supplied tileset contradicts the engine's own assignment for a map.
///
/// `None` means proceed, and it means that in three distinct cases: the map's extension is not one
/// the corpus classifies, the tileset is the engine's own for that class, or the tileset is not a
/// shipped name at all and so is presumed modded. Only a *shipped* tileset paired with the *wrong*
/// class is refused, because that is the only case where the engine's behaviour is known and the
/// caller is contradicting it.
pub fn tileset_mismatch(
    map_path: &std::path::Path,
    tile_set_path: &std::path::Path,
) -> Option<TileSetMismatch> {
    let class = MapClass::from_path(map_path)?;
    let supplied = tile_set_path.file_name()?.to_str()?;
    if !is_shipped_tileset(supplied) {
        return None;
    }
    let expected = engine_tileset_member(class);
    if supplied.eq_ignore_ascii_case(expected) {
        return None;
    }
    Some(TileSetMismatch {
        class,
        supplied: supplied.to_owned(),
        expected,
    })
}

fn parse_pair(value: &str, line: usize, name: &str) -> Result<(u32, u32), TileError> {
    let fields = csv_fields(value, line)?;
    if fields.len() != 2 {
        return Err(TileError::new(format!(
            "{name} on line {line} does not contain two values"
        )));
    }
    Ok((
        parse_u32(&fields[0], line, name)?,
        parse_u32(&fields[1], line, name)?,
    ))
}

fn parse_u32(value: &str, line: usize, name: &str) -> Result<u32, TileError> {
    value
        .trim()
        .parse()
        .map_err(|_| TileError::new(format!("invalid {name} on line {line}: {value}")))
}

fn csv_fields(value: &str, line: usize) -> Result<Vec<String>, TileError> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    for character in value.chars() {
        match character {
            '"' => quoted = !quoted,
            ',' if !quoted => {
                fields.push(field.trim().to_owned());
                field.clear();
            }
            _ => field.push(character),
        }
    }
    if quoted {
        return Err(TileError::new(format!(
            "unterminated quote in comma-separated data on line {line}"
        )));
    }
    fields.push(field.trim().to_owned());
    Ok(fields)
}

#[cfg(test)]
mod tests {
    use super::{
        Direction, NeighbourConstraint, Neighbourhood, Passability, TileChoice, TileSelector,
        TileSetDefinition,
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

    /// The engine's own assignment, per class, including the **case split** the installed corpus
    /// actually has.
    ///
    /// The `map/` directory ships 172 `.smp` and 165 `.SMP`. A case-sensitive classifier would
    /// call 165 combat maps unknown and hand them the world tileset, so an uppercase input is the
    /// input that makes this check fail, and it is here.
    #[test]
    fn the_engine_reads_combat_maps_through_tilesa01_and_world_maps_through_tilesb01() {
        use super::{MapClass, engine_tileset_member};
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
            MapClass::from_path(Path::new("map/aicave.smp")),
            Some(MapClass::Combat)
        );
        assert_eq!(
            MapClass::from_path(Path::new("map/URAK.scn")),
            Some(MapClass::World)
        );
        assert_eq!(MapClass::from_path(Path::new("URAK")), None);

        assert_eq!(engine_tileset_member(MapClass::Combat), "tilesa01.til");
        assert_eq!(engine_tileset_member(MapClass::World), "tilesb01.til");
    }

    /// A shipped tileset paired with the wrong map class is refused; a modded one is not.
    ///
    /// The three inputs that must make this fail are all here: a combat map handed the world
    /// tileset, a world map handed the combat tileset, and `aibldg01.til` handed to
    /// `AIBLDG01.SMP` -- the name-match that reads as obviously right and is not the engine's.
    #[test]
    fn a_shipped_tileset_for_the_wrong_map_class_is_a_mismatch_and_a_modded_one_is_not() {
        use super::{MapClass, tileset_mismatch};
        use std::path::Path;

        let mismatch = tileset_mismatch(
            Path::new("map/aicave.smp"),
            Path::new("til/tilesb01.til"),
        )
        .expect("the world tileset is not what the engine reads a combat map through");
        assert_eq!(mismatch.class, MapClass::Combat);
        assert_eq!(mismatch.supplied, "tilesb01.til");
        assert_eq!(mismatch.expected, "tilesa01.til");
        assert!(mismatch.to_string().contains("combattileset"));

        // The name-match trap: AIBLDG01.SMP alongside aibldg01.til.
        let named = tileset_mismatch(
            Path::new("map/AIBLDG01.SMP"),
            Path::new("til/aibldg01.til"),
        )
        .expect("a same-named building tileset is still not the combat tileset");
        assert_eq!(named.expected, "tilesa01.til");

        // A world map handed the combat tileset.
        assert_eq!(
            tileset_mismatch(Path::new("URAK.scn"), Path::new("tilesa01.til"))
                .map(|mismatch| mismatch.expected),
            Some("tilesb01.til")
        );

        // The engine's own pairings pass, in either case.
        assert!(tileset_mismatch(Path::new("a.smp"), Path::new("tilesa01.til")).is_none());
        assert!(tileset_mismatch(Path::new("A.SMP"), Path::new("TILESA01.TIL")).is_none());
        assert!(tileset_mismatch(Path::new("a.scn"), Path::new("tilesb01.til")).is_none());

        // A modded tileset is presumed deliberate and passes for either class.
        assert!(tileset_mismatch(Path::new("a.smp"), Path::new("mymod.til")).is_none());
        assert!(tileset_mismatch(Path::new("a.scn"), Path::new("mymod.til")).is_none());

        // An unclassified extension has no rule to contradict.
        assert!(tileset_mismatch(Path::new("a.dat"), Path::new("tilesb01.til")).is_none());
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
}
