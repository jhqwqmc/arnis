use crate::args::Args;
use crate::block_definitions::*;
use crate::progress::emit_gui_progress_update;
use colored::Colorize;
use fastanvil::Region;
use fastnbt::{LongArray, Value};
use fnv::FnvHashMap;
use indicatif::{ProgressBar, ProgressStyle};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Chunk {
    sections: Vec<Section>,
    x_pos: i32,
    z_pos: i32,
    #[serde(default)]
    is_light_on: u8,
    #[serde(flatten)]
    other: FnvHashMap<String, Value>,
}

#[derive(Serialize, Deserialize)]
struct Section {
    block_states: Blockstates,
    #[serde(rename = "Y")]
    y: i8,
    #[serde(flatten)]
    other: FnvHashMap<String, Value>,
}

#[derive(Serialize, Deserialize)]
struct Blockstates {
    palette: Vec<PaletteItem>,
    data: Option<LongArray>,
    #[serde(flatten)]
    other: FnvHashMap<String, Value>,
}

#[derive(Serialize, Deserialize)]
struct PaletteItem {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Properties")]
    properties: Option<Value>,
}

struct SectionToModify {
    blocks: [Block; 4096],
}

impl SectionToModify {
    fn get_block(&self, x: u8, y: u8, z: u8) -> Option<Block> {
        let b = self.blocks[Self::index(x, y, z)];
        if b == AIR {
            return None;
        }

        Some(b)
    }

    fn set_block(&mut self, x: u8, y: u8, z: u8, block: Block) {
        self.blocks[Self::index(x, y, z)] = block;
    }

    fn index(x: u8, y: u8, z: u8) -> usize {
        usize::from(y) % 16 * 256 + usize::from(z) * 16 + usize::from(x)
    }

    fn to_section(&self, y: i8) -> Section {
        let mut palette = self.blocks.to_vec();
        palette.sort();
        palette.dedup();

        let palette_lookup: FnvHashMap<_, _> = palette
            .iter()
            .enumerate()
            .map(|(k, v)| (v, i64::try_from(k).unwrap()))
            .collect();

        let mut bits_per_block = 4; // minimum allowed
        while (1 << bits_per_block) < palette.len() {
            bits_per_block += 1;
        }

        let mut data = vec![];

        let mut cur = 0;
        let mut cur_idx = 0;
        for block in &self.blocks {
            let p = palette_lookup[block];

            if cur_idx + bits_per_block > 64 {
                data.push(cur);
                cur = 0;
                cur_idx = 0;
            }

            cur |= p << cur_idx;
            cur_idx += bits_per_block;
        }

        if cur_idx > 0 {
            data.push(cur);
        }

        let palette = palette
            .iter()
            .map(|x| PaletteItem {
                name: x.name().to_string(),
                properties: x.properties(),
            })
            .collect();

        Section {
            block_states: Blockstates {
                palette,
                data: Some(LongArray::new(data)),
                other: FnvHashMap::default(),
            },
            y,
            other: FnvHashMap::default(),
        }
    }
}

impl Default for SectionToModify {
    fn default() -> Self {
        Self {
            blocks: [AIR; 4096],
        }
    }
}

#[derive(Default)]
struct ChunkToModify {
    sections: FnvHashMap<i8, SectionToModify>,
    other: FnvHashMap<String, Value>,
}

impl ChunkToModify {
    fn get_block(&self, x: u8, y: i32, z: u8) -> Option<Block> {
        let section_idx: i8 = (y >> 4).try_into().unwrap();

        let section = self.sections.get(&section_idx)?;

        section.get_block(x, (y & 15).try_into().unwrap(), z)
    }

    fn set_block(&mut self, x: u8, y: i32, z: u8, block: Block) {
        let section_idx: i8 = (y >> 4).try_into().unwrap();

        let section = self.sections.entry(section_idx).or_default();

        section.set_block(x, (y & 15).try_into().unwrap(), z, block);
    }

    fn sections(&self) -> impl Iterator<Item = Section> + '_ {
        self.sections.iter().map(|(y, s)| s.to_section(*y))
    }
}

#[derive(Default)]
struct RegionToModify {
    chunks: FnvHashMap<(i32, i32), ChunkToModify>,
}

impl RegionToModify {
    fn get_or_create_chunk(&mut self, x: i32, z: i32) -> &mut ChunkToModify {
        self.chunks.entry((x, z)).or_default()
    }

    fn get_chunk(&self, x: i32, z: i32) -> Option<&ChunkToModify> {
        self.chunks.get(&(x, z))
    }
}

#[derive(Default)]
struct WorldToModify {
    regions: FnvHashMap<(i32, i32), RegionToModify>,
}

impl WorldToModify {
    fn get_or_create_region(&mut self, x: i32, z: i32) -> &mut RegionToModify {
        self.regions.entry((x, z)).or_default()
    }

    fn get_region(&self, x: i32, z: i32) -> Option<&RegionToModify> {
        self.regions.get(&(x, z))
    }

    fn get_block(&self, x: i32, y: i32, z: i32) -> Option<Block> {
        let chunk_x: i32 = x >> 4;
        let chunk_z: i32 = z >> 4;
        let region_x: i32 = chunk_x >> 5;
        let region_z: i32 = chunk_z >> 5;

        let region: &RegionToModify = self.get_region(region_x, region_z)?;
        let chunk: &ChunkToModify = region.get_chunk(chunk_x & 31, chunk_z & 31)?;

        chunk.get_block(
            (x & 15).try_into().unwrap(),
            y,
            (z & 15).try_into().unwrap(),
        )
    }

    fn set_block(&mut self, x: i32, y: i32, z: i32, block: Block) {
        let chunk_x: i32 = x >> 4;
        let chunk_z: i32 = z >> 4;
        let region_x: i32 = chunk_x >> 5;
        let region_z: i32 = chunk_z >> 5;

        let region: &mut RegionToModify = self.get_or_create_region(region_x, region_z);
        let chunk: &mut ChunkToModify = region.get_or_create_chunk(chunk_x & 31, chunk_z & 31);

        chunk.set_block(
            (x & 15).try_into().unwrap(),
            y,
            (z & 15).try_into().unwrap(),
            block,
        );
    }
}

pub struct WorldEditor<'a> {
    region_dir: String,
    world: WorldToModify,
    scale_factor_x: f64,
    scale_factor_z: f64,
    args: &'a Args,
}

impl<'a> WorldEditor<'a> {
    /// Initializes the WorldEditor with the region directory and template region path.
    pub fn new(region_dir: &str, scale_factor_x: f64, scale_factor_z: f64, args: &'a Args) -> Self {
        Self {
            region_dir: region_dir.to_string(),
            world: WorldToModify::default(),
            scale_factor_x,
            scale_factor_z,
            args,
        }
    }

    /// Creates a region for the given region coordinates.
    fn create_region(&self, region_x: i32, region_z: i32) -> Region<File> {
        let out_path: String = format!("{}/r.{}.{}.mca", self.region_dir, region_x, region_z);

        const REGION_TEMPLATE: &[u8] = include_bytes!("../mcassets/region.template");

        let mut region_file: File = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&out_path)
            .expect("无法打开区域文件");

        region_file
            .write_all(REGION_TEMPLATE)
            .expect("无法写入区域模板");

        Region::from_stream(region_file).expect("加载区域失败")
    }

    pub fn get_max_coords(&self) -> (i32, i32) {
        (self.scale_factor_x as i32, self.scale_factor_x as i32)
    }

    // Unused and not tested
    /*pub fn block_at(&self, x: i32, y: i32, z: i32) -> bool {
        self.world.get_block(x, y, z).is_some()
    }*/

    #[allow(clippy::too_many_arguments)]
    pub fn set_sign(
        &mut self,
        line1: String,
        line2: String,
        line3: String,
        line4: String,
        x: i32,
        y: i32,
        z: i32,
        _rotation: i8,
    ) {
        let chunk_x = x >> 4;
        let chunk_z = z >> 4;
        let region_x = chunk_x >> 5;
        let region_z = chunk_z >> 5;

        let mut block_entities = HashMap::new();

        let messages = vec![
            Value::String(format!("\"{}\"", line1)),
            Value::String(format!("\"{}\"", line2)),
            Value::String(format!("\"{}\"", line3)),
            Value::String(format!("\"{}\"", line4)),
        ];

        let mut text_data = HashMap::new();
        text_data.insert("messages".to_string(), Value::List(messages));
        text_data.insert("color".to_string(), Value::String("black".to_string()));
        text_data.insert("has_glowing_text".to_string(), Value::Byte(0));

        block_entities.insert("front_text".to_string(), Value::Compound(text_data));
        block_entities.insert(
            "id".to_string(),
            Value::String("minecraft:sign".to_string()),
        );
        block_entities.insert("is_waxed".to_string(), Value::Byte(0));
        block_entities.insert("keepPacked".to_string(), Value::Byte(0));
        block_entities.insert("x".to_string(), Value::Int(x));
        block_entities.insert("y".to_string(), Value::Int(y));
        block_entities.insert("z".to_string(), Value::Int(z));

        let region: &mut RegionToModify = self.world.get_or_create_region(region_x, region_z);
        let chunk: &mut ChunkToModify = region.get_or_create_chunk(chunk_x & 31, chunk_z & 31);

        if let Some(chunk_data) = chunk.other.get_mut("block_entities") {
            if let Value::List(entities) = chunk_data {
                entities.push(Value::Compound(block_entities));
            }
        } else {
            chunk.other.insert(
                "block_entities".to_string(),
                Value::List(vec![Value::Compound(block_entities)]),
            );
        }

        self.set_block(SIGN, x, y, z, None, None);
    }

    /// Sets a block of the specified type at the given coordinates.
    pub fn set_block(
        &mut self,
        block: Block,
        x: i32,
        y: i32,
        z: i32,
        override_whitelist: Option<&[Block]>,
        override_blacklist: Option<&[Block]>,
    ) {
        // Check if coordinates are within bounds
        if x < 0 || x > self.scale_factor_x as i32 || z < 0 || z > self.scale_factor_z as i32 {
            return;
        }

        let should_insert = if let Some(existing_block) = self.world.get_block(x, y, z) {
            // Check against whitelist and blacklist
            if let Some(whitelist) = override_whitelist {
                whitelist
                    .iter()
                    .any(|whitelisted_block: &Block| whitelisted_block.id() == existing_block.id())
            } else if let Some(blacklist) = override_blacklist {
                !blacklist
                    .iter()
                    .any(|blacklisted_block: &Block| blacklisted_block.id() == existing_block.id())
            } else {
                false
            }
        } else {
            true
        };

        if should_insert {
            self.world.set_block(x, y, z, block);
        }
    }

    /// Fills a cuboid area with the specified block between two coordinates.
    #[allow(clippy::too_many_arguments)]
    pub fn fill_blocks(
        &mut self,
        block: Block,
        x1: i32,
        y1: i32,
        z1: i32,
        x2: i32,
        y2: i32,
        z2: i32,
        override_whitelist: Option<&[Block]>,
        override_blacklist: Option<&[Block]>,
    ) {
        let (min_x, max_x) = if x1 < x2 { (x1, x2) } else { (x2, x1) };
        let (min_y, max_y) = if y1 < y2 { (y1, y2) } else { (y2, y1) };
        let (min_z, max_z) = if z1 < z2 { (z1, z2) } else { (z2, z1) };

        for x in min_x..=max_x {
            for y in min_y..=max_y {
                for z in min_z..=max_z {
                    self.set_block(block, x, y, z, override_whitelist, override_blacklist);
                }
            }
        }
    }

    /// Checks for a block at the given coordinates.
    pub fn check_for_block(
        &self,
        x: i32,
        y: i32,
        z: i32,
        whitelist: Option<&[Block]>,
        blacklist: Option<&[Block]>,
    ) -> bool {
        // Retrieve the chunk modification map
        if let Some(existing_block) = self.world.get_block(x, y, z) {
            // Check against whitelist and blacklist
            if let Some(whitelist) = whitelist {
                if whitelist
                    .iter()
                    .any(|whitelisted_block: &Block| whitelisted_block.id() == existing_block.id())
                {
                    return true; // Block is in whitelist
                }
            }
            if let Some(blacklist) = blacklist {
                if blacklist
                    .iter()
                    .any(|blacklisted_block: &Block| blacklisted_block.id() == existing_block.id())
                {
                    return true; // Block is in blacklist
                }
            }
        }

        false
    }

    /// Saves all changes made to the world by writing modified chunks to the appropriate region files.
    pub fn save(&mut self) {
        println!("{} 保存世界...", "[5/5]".bold());
        emit_gui_progress_update(90.0, "保存世界...");

        let _debug: bool = self.args.debug;
        let total_regions: u64 = self.world.regions.len() as u64;

        let save_pb: ProgressBar = ProgressBar::new(total_regions);
        save_pb.set_style(
            ProgressStyle::default_bar()
                .template(
                    "{spinner:.green} [{elapsed_precise}] [{bar:45}] {pos}/{len} 区域 ({eta})",
                )
                .unwrap()
                .progress_chars("█▓░"),
        );

        let total_steps: f64 = 9.0;
        let progress_increment_save: f64 = total_steps / total_regions as f64;
        let mut current_progress_save: f64 = 90.0;
        let mut last_emitted_progress: f64 = current_progress_save;

        for ((region_x, region_z), region_to_modify) in &self.world.regions {
            let mut region: Region<File> = self.create_region(*region_x, *region_z);

            for chunk_x in 0..32 {
                for chunk_z in 0..32 {
                    let data: Vec<u8> = region
                        .read_chunk(chunk_x as usize, chunk_z as usize)
                        .unwrap()
                        .unwrap();

                    let mut chunk: Chunk = fastnbt::from_bytes(&data).unwrap();

                    if let Some(chunk_to_modify) = region_to_modify.get_chunk(chunk_x, chunk_z) {
                        chunk.sections = chunk_to_modify.sections().collect();
                        chunk.other.extend(chunk_to_modify.other.clone());
                    }

                    chunk.x_pos = chunk_x + region_x * 32;
                    chunk.z_pos = chunk_z + region_z * 32;
                    chunk.is_light_on = 0; // Force minecraft to recompute

                    let ser: Vec<u8> = fastnbt::to_bytes(&chunk).unwrap();

                    // Write chunk data back to the correct location, ensuring correct chunk coordinates
                    let expected_chunk_location: (usize, usize) =
                        ((chunk_x as usize) & 31, (chunk_z as usize) & 31);
                    region
                        .write_chunk(expected_chunk_location.0, expected_chunk_location.1, &ser)
                        .unwrap();
                }
            }

            save_pb.inc(1);

            current_progress_save += progress_increment_save;
            if (current_progress_save - last_emitted_progress).abs() > 0.25 {
                emit_gui_progress_update(current_progress_save, "保存世界...");
                last_emitted_progress = current_progress_save;
            }
        }

        save_pb.finish();
    }
}
