//! Tilesets, tilemaps, autotiling and tilemap export.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{Bitmap, Rect};

/// How map cells are laid out on screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GridKind {
    #[default]
    Rectangular,
    /// Diamond-shaped isometric cells.
    Isometric,
    /// Hexagons with a pointed top; odd rows are shifted right.
    HexPointy,
    /// Hexagons with a flat top; odd columns are shifted down.
    HexFlat,
}

impl GridKind {
    pub const ALL: [GridKind; 4] = [Self::Rectangular, Self::Isometric, Self::HexPointy, Self::HexFlat];

    pub fn name(self) -> &'static str {
        match self {
            Self::Rectangular => "Rectangular",
            Self::Isometric => "Isometric",
            Self::HexPointy => "Hexagonal (pointy)",
            Self::HexFlat => "Hexagonal (flat)",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Collision {
    #[default]
    None,
    Solid,
    /// A box inside the tile, in tile pixels.
    Box { x: i32, y: i32, w: i32, h: i32 },
}

/// Game data attached to a tile.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TileProps {
    pub collision: Collision,
    /// The terrain this tile belongs to (for autotiling).
    pub terrain: Option<usize>,
    pub material: String,
    /// Relative chance of this variant being picked by random and auto tiling.
    pub weight: f32,
    pub custom: BTreeMap<String, String>,
}

impl Default for TileProps {
    fn default() -> Self {
        Self {
            collision: Collision::None,
            terrain: None,
            material: String::new(),
            weight: 1.0,
            custom: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tile {
    /// Stored in the document's image data, not in JSON.
    #[serde(skip, default = "empty_bitmap")]
    pub image: Bitmap,
    #[serde(default)]
    pub props: TileProps,
}

fn empty_bitmap() -> Bitmap {
    Bitmap::new(0, 0)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AutotileMode {
    /// Looks at the 4 edge neighbours: 16 variants.
    #[default]
    Edge4,
    /// Looks at all 8 neighbours (corners only where both edges match): 47 variants.
    Blob8,
}

/// A kind of ground (grass, water...) with the tile to use for each neighbour pattern.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Terrain {
    pub name: String,
    pub mode: AutotileMode,
    /// Neighbour bitmask to candidate tile ids.
    pub rules: BTreeMap<u8, Vec<usize>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tileset {
    pub tile_w: u32,
    pub tile_h: u32,
    pub tiles: Vec<Tile>,
    #[serde(default)]
    pub terrains: Vec<Terrain>,
}

/// A placed tile: which one and how it is flipped and turned.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TileRef {
    pub id: usize,
    #[serde(default)]
    pub flip_h: bool,
    #[serde(default)]
    pub flip_v: bool,
    /// Quarter turns clockwise (0..=3).
    #[serde(default)]
    pub rotation: u8,
}

impl TileRef {
    pub fn new(id: usize) -> Self {
        Self {
            id,
            ..Default::default()
        }
    }

    /// The tile's pixels with the flips and rotation applied.
    pub fn image(&self, set: &Tileset) -> Option<Bitmap> {
        let mut img = set.tiles.get(self.id)?.image.clone();
        for _ in 0..self.rotation % 4 {
            img = img.rotated_90(true);
        }
        if self.flip_h {
            img = img.flipped_horizontal();
        }
        if self.flip_v {
            img = img.flipped_vertical();
        }
        Some(img)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tilemap {
    pub grid: GridKind,
    pub cols: u32,
    pub rows: u32,
    /// Canvas offset of the map's top-left corner.
    #[serde(default)]
    pub offset: (i32, i32),
    /// Row by row; `None` is an empty cell.
    pub cells: Vec<Option<TileRef>>,
}

impl Tileset {
    pub fn new(tile_w: u32, tile_h: u32) -> Self {
        Self {
            tile_w: tile_w.max(1),
            tile_h: tile_h.max(1),
            tiles: Vec::new(),
            terrains: Vec::new(),
        }
    }

    /// Cuts an image into tiles: a grid of `tile_w × tile_h` cells starting at `offset`,
    /// `spacing` pixels apart. Empty cells are skipped when asked, and identical tiles are
    /// added only once when `dedupe`.
    pub fn slice(
        image: &Bitmap,
        tile_w: u32,
        tile_h: u32,
        offset: (i32, i32),
        spacing: (i32, i32),
        skip_empty: bool,
        dedupe: bool,
    ) -> Vec<Bitmap> {
        let mut out: Vec<Bitmap> = Vec::new();
        let (tw, th) = (tile_w.max(1) as i32, tile_h.max(1) as i32);
        let mut y = offset.1;
        while y + th <= image.height() as i32 {
            let mut x = offset.0;
            while x + tw <= image.width() as i32 {
                let tile = image.crop(Rect::new(x, y, tw, th));
                let empty = tile.is_clear();
                if !(skip_empty && empty) && !(dedupe && out.contains(&tile)) {
                    out.push(tile);
                }
                x += tw + spacing.0;
            }
            y += th + spacing.1;
        }
        out
    }

    pub fn add_tile(&mut self, image: Bitmap) -> usize {
        let image = if (image.width(), image.height()) == (self.tile_w, self.tile_h) {
            image
        } else {
            image.resized(self.tile_w, self.tile_h)
        };
        self.tiles.push(Tile {
            image,
            props: TileProps::default(),
        });
        self.tiles.len() - 1
    }

    /// Picks one of `candidates` by weight, deterministically from `seed`.
    pub fn pick_weighted(&self, candidates: &[usize], seed: u64) -> Option<usize> {
        let weights: Vec<f32> = candidates
            .iter()
            .map(|id| self.tiles.get(*id).map_or(0.0, |t| t.props.weight.max(0.0)))
            .collect();
        let total: f32 = weights.iter().sum();
        if candidates.is_empty() {
            return None;
        }
        if total <= 0.0 {
            return Some(candidates[(seed % candidates.len() as u64) as usize]);
        }
        let mut r = (hash(seed) % 1_000_000) as f32 / 1_000_000.0 * total;
        for (id, w) in candidates.iter().zip(&weights) {
            if r < *w {
                return Some(*id);
            }
            r -= w;
        }
        candidates.last().copied()
    }

    /// Lays out all tiles in an image `columns` wide, for saving.
    pub fn atlas(&self, columns: u32) -> Bitmap {
        let columns = columns.max(1);
        let rows = (self.tiles.len() as u32).div_ceil(columns).max(1);
        let mut out = Bitmap::new(columns * self.tile_w, rows * self.tile_h);
        for (i, tile) in self.tiles.iter().enumerate() {
            let (c, r) = (i as u32 % columns, i as u32 / columns);
            out.blit(&tile.image, (c * self.tile_w) as i32, (r * self.tile_h) as i32);
        }
        out
    }

    /// Reads tile images back from an [`Self::atlas`] image.
    pub fn load_atlas(&mut self, atlas: &Bitmap, columns: u32) {
        let columns = columns.max(1);
        for (i, tile) in self.tiles.iter_mut().enumerate() {
            let (c, r) = (i as u32 % columns, i as u32 / columns);
            tile.image = atlas.crop(Rect::new(
                (c * self.tile_w) as i32,
                (r * self.tile_h) as i32,
                self.tile_w as i32,
                self.tile_h as i32,
            ));
        }
    }
}

fn hash(mut x: u64) -> u64 {
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    x ^ (x >> 33)
}

/// Neighbour bits for autotiling.
pub const N: u8 = 1;
pub const E: u8 = 2;
pub const S: u8 = 4;
pub const W: u8 = 8;
pub const NE: u8 = 16;
pub const SE: u8 = 32;
pub const SW: u8 = 64;
pub const NW: u8 = 128;

/// Drops corner bits whose two neighbouring edges are not both set (the 47-tile blob set).
pub fn reduce_blob(mask: u8) -> u8 {
    let mut m = mask & 0x0f;
    let pairs = [(NE, N, E), (SE, S, E), (SW, S, W), (NW, N, W)];
    for (corner, a, b) in pairs {
        if mask & corner != 0 && mask & a != 0 && mask & b != 0 {
            m |= corner;
        }
    }
    m
}

impl Tilemap {
    pub fn new(grid: GridKind, cols: u32, rows: u32) -> Self {
        Self {
            grid,
            cols,
            rows,
            offset: (0, 0),
            cells: vec![None; (cols * rows) as usize],
        }
    }

    pub fn get(&self, col: i32, row: i32) -> Option<TileRef> {
        if col < 0 || row < 0 || col as u32 >= self.cols || row as u32 >= self.rows {
            return None;
        }
        self.cells[(row as u32 * self.cols + col as u32) as usize]
    }

    pub fn set(&mut self, col: i32, row: i32, tile: Option<TileRef>) {
        if col >= 0 && row >= 0 && (col as u32) < self.cols && (row as u32) < self.rows {
            self.cells[(row as u32 * self.cols + col as u32) as usize] = tile;
        }
    }

    /// Canvas position of a cell's top-left corner.
    pub fn cell_origin(&self, set: &Tileset, col: i32, row: i32) -> (i32, i32) {
        let (tw, th) = (set.tile_w as i32, set.tile_h as i32);
        let (x, y) = match self.grid {
            GridKind::Rectangular => (col * tw, row * th),
            GridKind::Isometric => {
                let origin = (self.rows as i32 - 1) * tw / 2;
                ((col - row) * tw / 2 + origin, (col + row) * th / 2)
            }
            GridKind::HexPointy => (col * tw + if row % 2 == 1 { tw / 2 } else { 0 }, row * th * 3 / 4),
            GridKind::HexFlat => (col * tw * 3 / 4, row * th + if col % 2 == 1 { th / 2 } else { 0 }),
        };
        (x + self.offset.0, y + self.offset.1)
    }

    /// The cell under a canvas pixel (nearest cell centre for non-rectangular grids).
    pub fn cell_at(&self, set: &Tileset, x: i32, y: i32) -> Option<(i32, i32)> {
        let (tw, th) = (set.tile_w as i32, set.tile_h as i32);
        if self.grid == GridKind::Rectangular {
            let (cx, cy) = ((x - self.offset.0).div_euclid(tw), (y - self.offset.1).div_euclid(th));
            return (cx >= 0 && cy >= 0 && (cx as u32) < self.cols && (cy as u32) < self.rows)
                .then_some((cx, cy));
        }
        let mut best: Option<((i32, i32), i64)> = None;
        for row in 0..self.rows as i32 {
            for col in 0..self.cols as i32 {
                let (ox, oy) = self.cell_origin(set, col, row);
                let (cx, cy) = (ox + tw / 2, oy + th / 2);
                let d = i64::from(x - cx).pow(2) * i64::from(th).pow(2)
                    + i64::from(y - cy).pow(2) * i64::from(tw).pow(2);
                if best.is_none_or(|(_, bd)| d < bd) {
                    best = Some(((col, row), d));
                }
            }
        }
        let ((col, row), d) = best?;
        // Only accept points reasonably inside the cell.
        (d <= i64::from(tw * th).pow(2) / 4).then_some((col, row))
    }

    /// Draws the map into a canvas-sized image.
    pub fn render(&self, set: &Tileset, width: u32, height: u32) -> Bitmap {
        let mut out = Bitmap::new(width, height);
        // Draw back to front so isometric tiles overlap correctly.
        for row in 0..self.rows as i32 {
            for col in 0..self.cols as i32 {
                if let Some(tile) = self.get(col, row)
                    && let Some(img) = tile.image(set)
                {
                    let (x, y) = self.cell_origin(set, col, row);
                    out.draw(&img, x, y);
                }
            }
        }
        out
    }

    /// Which neighbours of a cell share a terrain, as N/E/S/W(/corner) bits.
    pub fn neighbour_mask(&self, set: &Tileset, col: i32, row: i32, terrain: usize, mode: AutotileMode) -> u8 {
        let is = |c: i32, r: i32| {
            self.get(c, r)
                .and_then(|t| set.tiles.get(t.id))
                .is_some_and(|t| t.props.terrain == Some(terrain))
        };
        let mut m = 0;
        for (bit, dc, dr) in [(N, 0, -1), (E, 1, 0), (S, 0, 1), (W, -1, 0)] {
            if is(col + dc, row + dr) {
                m |= bit;
            }
        }
        if mode == AutotileMode::Blob8 {
            for (bit, dc, dr) in [(NE, 1, -1), (SE, 1, 1), (SW, -1, 1), (NW, -1, -1)] {
                if is(col + dc, row + dr) {
                    m |= bit;
                }
            }
            m = reduce_blob(m);
        }
        m
    }

    /// Paints a terrain at a cell and re-picks the variants of it and its neighbours.
    pub fn paint_terrain(&mut self, set: &Tileset, terrain: usize, col: i32, row: i32) {
        let Some(t) = set.terrains.get(terrain) else { return };
        let Some(any) = set
            .tiles
            .iter()
            .position(|tile| tile.props.terrain == Some(terrain))
        else {
            return;
        };
        self.set(col, row, Some(TileRef::new(any)));
        for dr in -1..=1 {
            for dc in -1..=1 {
                let (c, r) = (col + dc, row + dr);
                let here = self
                    .get(c, r)
                    .and_then(|tr| set.tiles.get(tr.id))
                    .is_some_and(|tile| tile.props.terrain == Some(terrain));
                if !here {
                    continue;
                }
                let mask = self.neighbour_mask(set, c, r, terrain, t.mode);
                let candidates = t
                    .rules
                    .get(&mask)
                    .or_else(|| t.rules.get(&0x0f))
                    .cloned()
                    .unwrap_or_else(|| vec![any]);
                let seed = (c as u64) << 32 ^ r as u64;
                if let Some(id) = set.pick_weighted(&candidates, seed) {
                    self.set(c, r, Some(TileRef::new(id)));
                }
            }
        }
    }

    /// Places a block of tiles with its top-left at a cell.
    pub fn stamp(&mut self, col: i32, row: i32, pattern: &[Vec<Option<TileRef>>]) {
        for (dr, line) in pattern.iter().enumerate() {
            for (dc, tile) in line.iter().enumerate() {
                if let Some(tile) = tile {
                    self.set(col + dc as i32, row + dr as i32, Some(*tile));
                }
            }
        }
    }

    /// Engine-neutral JSON: grid, size, and every placed tile with its properties.
    pub fn to_json(&self, set: &Tileset, name: &str) -> Value {
        let tiles: Vec<Value> = (0..self.rows as i32)
            .flat_map(|r| (0..self.cols as i32).map(move |c| (c, r)))
            .filter_map(|(c, r)| {
                let t = self.get(c, r)?;
                let (x, y) = self.cell_origin(set, c, r);
                let props = set.tiles.get(t.id).map(|tile| &tile.props);
                Some(json!({
                    "col": c, "row": r, "x": x, "y": y,
                    "tile": t.id, "flipH": t.flip_h, "flipV": t.flip_v, "rotation": u32::from(t.rotation) * 90,
                    "collision": props.map(|p| serde_json::to_value(&p.collision).unwrap_or(Value::Null)),
                    "material": props.map(|p| p.material.clone()),
                    "properties": props.map(|p| p.custom.clone()),
                }))
            })
            .collect();
        json!({
            "name": name,
            "grid": format!("{:?}", self.grid),
            "cols": self.cols, "rows": self.rows,
            "tileWidth": set.tile_w, "tileHeight": set.tile_h,
            "tiles": tiles,
        })
    }

    /// A Tiled map (`.tmj`) with one tile layer. `image` is the tileset image path.
    pub fn to_tiled(&self, set: &Tileset, image: &str, columns: u32) -> Value {
        const FLIP_H: u32 = 0x8000_0000;
        const FLIP_V: u32 = 0x4000_0000;
        const FLIP_D: u32 = 0x2000_0000;
        let data: Vec<u32> = self
            .cells
            .iter()
            .map(|c| match c {
                None => 0,
                Some(t) => {
                    // Tiled encodes rotation as flips.
                    let (mut h, mut v, mut d) = (t.flip_h, t.flip_v, false);
                    match t.rotation % 4 {
                        1 => {
                            d = !d;
                            std::mem::swap(&mut h, &mut v);
                            h = !h;
                        }
                        2 => {
                            h = !h;
                            v = !v;
                        }
                        3 => {
                            d = !d;
                            std::mem::swap(&mut h, &mut v);
                            v = !v;
                        }
                        _ => {}
                    }
                    let mut gid = t.id as u32 + 1;
                    if h {
                        gid |= FLIP_H;
                    }
                    if v {
                        gid |= FLIP_V;
                    }
                    if d {
                        gid |= FLIP_D;
                    }
                    gid
                }
            })
            .collect();
        let orientation = match self.grid {
            GridKind::Rectangular => "orthogonal",
            GridKind::Isometric => "isometric",
            GridKind::HexPointy | GridKind::HexFlat => "hexagonal",
        };
        let tiles: Vec<Value> = set
            .tiles
            .iter()
            .enumerate()
            .filter(|(_, t)| t.props != TileProps::default())
            .map(|(i, t)| {
                let mut props: Vec<Value> = t
                    .props
                    .custom
                    .iter()
                    .map(|(k, v)| json!({"name": k, "type": "string", "value": v}))
                    .collect();
                if !t.props.material.is_empty() {
                    props.push(json!({"name": "material", "type": "string", "value": t.props.material}));
                }
                props.push(json!({"name": "collision", "type": "string", "value": format!("{:?}", t.props.collision)}));
                json!({"id": i, "probability": t.props.weight, "properties": props})
            })
            .collect();
        let mut map = json!({
            "type": "map", "version": "1.10", "orientation": orientation, "renderorder": "right-down",
            "width": self.cols, "height": self.rows, "tilewidth": set.tile_w, "tileheight": set.tile_h,
            "infinite": false, "nextlayerid": 2, "nextobjectid": 1,
            "layers": [{
                "id": 1, "name": "Tiles", "type": "tilelayer", "visible": true, "opacity": 1,
                "x": 0, "y": 0, "width": self.cols, "height": self.rows, "data": data,
            }],
            "tilesets": [{
                "firstgid": 1, "name": "tiles", "image": image,
                "tilewidth": set.tile_w, "tileheight": set.tile_h, "columns": columns,
                "tilecount": set.tiles.len(), "margin": 0, "spacing": 0,
                "imagewidth": columns * set.tile_w,
                "imageheight": (set.tiles.len() as u32).div_ceil(columns.max(1)) * set.tile_h,
                "tiles": tiles,
            }],
        });
        if matches!(self.grid, GridKind::HexPointy | GridKind::HexFlat) {
            map["staggeraxis"] = json!(if self.grid == GridKind::HexPointy { "y" } else { "x" });
            map["staggerindex"] = json!("odd");
            map["hexsidelength"] = json!(if self.grid == GridKind::HexPointy { set.tile_h / 2 } else { set.tile_w / 2 });
        }
        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set_with(n: usize) -> Tileset {
        let mut set = Tileset::new(2, 2);
        for i in 0..n {
            set.add_tile(Bitmap::filled(2, 2, [i as u8 * 10, 0, 0, 255]));
        }
        set
    }

    #[test]
    fn slicing_skips_empty_and_duplicates() {
        let mut img = Bitmap::new(6, 2);
        img.blit(&Bitmap::filled(2, 2, [1, 1, 1, 255]), 0, 0);
        img.blit(&Bitmap::filled(2, 2, [1, 1, 1, 255]), 4, 0);
        assert_eq!(Tileset::slice(&img, 2, 2, (0, 0), (0, 0), false, false).len(), 3);
        assert_eq!(Tileset::slice(&img, 2, 2, (0, 0), (0, 0), true, false).len(), 2);
        assert_eq!(Tileset::slice(&img, 2, 2, (0, 0), (0, 0), true, true).len(), 1);
    }

    #[test]
    fn render_and_flip() {
        let mut set = Tileset::new(2, 1);
        let mut img = Bitmap::new(2, 1);
        img.set(0, 0, [255, 0, 0, 255]);
        set.add_tile(img);
        let mut map = Tilemap::new(GridKind::Rectangular, 2, 1);
        map.set(1, 0, Some(TileRef { id: 0, flip_h: true, ..Default::default() }));
        let out = map.render(&set, 4, 1);
        assert_eq!(out.get(3, 0), Some([255, 0, 0, 255]));
        assert_eq!(out.get(2, 0), Some(crate::TRANSPARENT));
        assert_eq!(map.cell_at(&set, 3, 0), Some((1, 0)));
    }

    #[test]
    fn edge_autotile_picks_by_neighbours() {
        let mut set = set_with(3);
        for t in &mut set.tiles {
            t.props.terrain = Some(0);
        }
        let mut rules = BTreeMap::new();
        rules.insert(0, vec![0]); // alone
        rules.insert(E, vec![1]); // something to the east
        rules.insert(W, vec![2]); // something to the west
        set.terrains.push(Terrain {
            name: "grass".into(),
            mode: AutotileMode::Edge4,
            rules,
        });
        let mut map = Tilemap::new(GridKind::Rectangular, 3, 1);
        map.paint_terrain(&set, 0, 0, 0);
        assert_eq!(map.get(0, 0).unwrap().id, 0);
        map.paint_terrain(&set, 0, 1, 0);
        assert_eq!(map.get(0, 0).unwrap().id, 1);
        assert_eq!(map.get(1, 0).unwrap().id, 2);
    }

    #[test]
    fn blob_reduction_and_weights() {
        assert_eq!(reduce_blob(NE | N), N);
        assert_eq!(reduce_blob(NE | N | E), NE | N | E);
        let mut set = set_with(2);
        set.tiles[0].props.weight = 0.0;
        for seed in 0..20 {
            assert_eq!(set.pick_weighted(&[0, 1], seed), Some(1));
        }
    }

    #[test]
    fn tiled_export_encodes_flips() {
        let set = set_with(1);
        let mut map = Tilemap::new(GridKind::Rectangular, 1, 1);
        map.set(0, 0, Some(TileRef { id: 0, flip_h: true, ..Default::default() }));
        let json = map.to_tiled(&set, "tiles.png", 1);
        assert_eq!(json["layers"][0]["data"][0], json!(0x8000_0001u32));
    }
}
