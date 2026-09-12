use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerrainTypeDefinition {
    pub index: u32,
    pub palette_color: u32,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TileDefinition {
    pub index: u32,
    pub terrain_type: u32,
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
                    let definition = TerrainTypeDefinition {
                        index,
                        palette_color,
                        description: fields[2].clone(),
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
                    let definition = TileDefinition {
                        index,
                        terrain_type,
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
    use super::TileSetDefinition;

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

    #[test]
    fn rejects_tiles_outside_the_declared_atlas() {
        let source = b"LBM=x.lbm\nTILES=1,1\nTILESIZE=32,32\nTILE=1,0\n";

        assert_eq!(
            TileSetDefinition::parse(source).unwrap_err().to_string(),
            "tile 1 exceeds declared atlas capacity 1"
        );
    }
}
