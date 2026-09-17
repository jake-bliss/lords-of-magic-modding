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
    /// `None` is a neighbour off the edge of the map, and it **satisfies every constraint**. That
    /// is an assumption, not a measurement: the `terrainrings` blobs are all interior, so no saved
    /// artifact says what the engine reads past a map edge. Refusing every paint that touches an
    /// edge would be the alternative; this writer instead records the count of off-map neighbours
    /// so a caller can see that a decision leaned on it.
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
    pub neighbours: [NeighbourConstraint; 8],
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
            pattern_index: None,
        }
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
    pub fn accepts(&self, neighbours: &Neighbourhood) -> bool {
        Direction::ALL
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
    /// Several match equally, so the engine would draw among them at random.
    ///
    /// `chosen` is this writer's [`TileSelector`] applied to `candidates`. It is a legal tile for
    /// the neighbourhood, not the tile the engine would have written.
    Ambiguous { chosen: u32, candidates: Vec<u32> },
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
            Self::Unique(tile) => Some(*tile),
            Self::Ambiguous { chosen, .. } => Some(*chosen),
            Self::NoCandidate => None,
        }
    }

    pub fn is_ambiguous(&self) -> bool {
        matches!(self, Self::Ambiguous { .. })
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
                    let trailing = |position: usize, name: &str| -> Result<Option<u32>, TileError> {
                        fields
                            .get(position)
                            .filter(|field| !field.is_empty())
                            .map(|field| parse_u32(field, line_number, name))
                            .transpose()
                    };
                    let definition = TerrainTypeDefinition {
                        index,
                        palette_color,
                        description: fields[2].clone(),
                        passability: trailing(3, "terrain passability flag")?
                            .map(Passability::from_value),
                        min_elevation: trailing(4, "terrain minimum elevation")?,
                        max_elevation: trailing(5, "terrain maximum elevation")?,
                        movement_cost: trailing(8, "terrain movement cost")?,
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
                    // A line that stops before the eight neighbour columns constrains nothing,
                    // which is what `Any` means. Partial lines are not rejected: the point of the
                    // parser is to read what shipped, and `tilesa01.til` is already a different
                    // shape from `tilesb01.til`.
                    let mut neighbours =
                        std::array::from_fn(|_| NeighbourConstraint::Any);
                    for (slot, position) in neighbours.iter_mut().zip(2..10) {
                        if let Some(field) = fields.get(position).filter(|f| !f.is_empty()) {
                            *slot = NeighbourConstraint::parse(field, line_number)?;
                        }
                    }
                    let pattern_index = fields
                        .get(10)
                        .filter(|field| !field.is_empty())
                        .map(|field| parse_u32(field, line_number, "tile pattern index"))
                        .transpose()?;
                    let definition = TileDefinition {
                        index,
                        terrain_type,
                        neighbours,
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
    pub fn candidates(&self, terrain_type: u32, neighbours: &Neighbourhood) -> Vec<u32> {
        self.tiles
            .values()
            .filter(|tile| tile.terrain_type == terrain_type && tile.accepts(neighbours))
            .map(|tile| tile.index)
            .collect()
    }

    /// Re-select a cell's tile: the tile a cell of `terrain_type` should hold given `neighbours`.
    ///
    /// **Derived, 2026-09-17.** Applied to the `terrainrings` captures this reproduces the engine
    /// exactly wherever the constraints decide: over all 121 background-by-painted rows, **2,084 of
    /// 2,084** cells whose candidate set was a single tile -- every transition ring cell and every
    /// perimeter cell of every painted region -- match the tile the engine wrote, with zero
    /// mismatches. Where several candidates match, the engine's own choice is a random draw and is
    /// not reproducible; in all 253 such cells the engine's tile was nonetheless *within* the
    /// candidate set, so the set is right even though the draw is not.
    pub fn select_tile(
        &self,
        terrain_type: u32,
        neighbours: &Neighbourhood,
        selector: TileSelector,
        at: (u32, u32),
    ) -> TileChoice {
        let candidates = self.candidates(terrain_type, neighbours);
        match candidates.len() {
            0 => TileChoice::NoCandidate,
            1 => TileChoice::Unique(candidates[0]),
            _ => TileChoice::Ambiguous {
                chosen: selector.choose(&candidates, at.0, at.1),
                candidates,
            },
        }
    }
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

    /// Selection reports the three outcomes it actually has, and the seeded selector stays inside
    /// the candidate set.
    #[test]
    fn selection_distinguishes_unique_ambiguous_and_no_candidate() {
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

        // Two interior tiles match a fully surrounded cell.
        assert_eq!(
            tile_set.select_tile(1, &all_grass, TileSelector::LowestSlot, (0, 0)),
            TileChoice::Ambiguous { chosen: 0, candidates: vec![0, 1] }
        );
        // One tile matches a cell with terrain 2 to its north.
        let mut north_is_two = all_grass;
        north_is_two.set(Direction::North, Some(2));
        assert_eq!(
            tile_set.select_tile(1, &north_is_two, TileSelector::LowestSlot, (0, 0)),
            TileChoice::Unique(2)
        );
        // Terrain 9's only tile needs all-9 neighbours, and gets none.
        assert_eq!(
            tile_set.select_tile(9, &all_grass, TileSelector::LowestSlot, (0, 0)),
            TileChoice::NoCandidate
        );
        // A seeded choice is still a candidate, and the same seed and cell give the same tile.
        for seed in 0..32 {
            let choice = tile_set.select_tile(1, &all_grass, TileSelector::Seeded(seed), (3, 4));
            let tile = choice.tile().unwrap();
            assert!([0, 1].contains(&tile), "seed {seed} chose {tile}");
            assert_eq!(
                tile_set
                    .select_tile(1, &all_grass, TileSelector::Seeded(seed), (3, 4))
                    .tile(),
                Some(tile)
            );
        }
    }

    #[test]
    fn rejects_tiles_outside_the_declared_atlas() {
        let source = b"LBM=x.lbm\nTILES=1,1\nTILESIZE=32,32\nTILE=1,0\n";

        assert_eq!(
            TileSetDefinition::parse(source).unwrap_err().to_string(),
            "tile 1 exceeds declared atlas capacity 1"
        );
    }
}
