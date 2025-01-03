#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod args;
mod block_definitions;
mod bresenham;
mod colors;
mod data_processing;
mod element_processing;
mod floodfill;
mod osm_parser;
mod progress;
mod retrieve_data;
mod version_check;
mod world_editor;

use args::Args;
use clap::Parser;
use colored::*;
use fastnbt::Value;
use flate2::read::GzDecoder;
use fs2::FileExt;
use rfd::FileDialog;
use std::{
    env,
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
};

fn print_banner() {
    let version: &str = env!("CARGO_PKG_VERSION");
    let repository: &str = env!("CARGO_PKG_REPOSITORY");
    println!(
        r#"
        ▄████████    ▄████████ ███▄▄▄▄    ▄█     ▄████████
        ███    ███   ███    ███ ███▀▀▀██▄ ███    ███    ███
        ███    ███   ███    ███ ███   ███ ███▌   ███    █▀
        ███    ███  ▄███▄▄▄▄██▀ ███   ███ ███▌   ███
      ▀███████████ ▀▀███▀▀▀▀▀   ███   ███ ███▌ ▀███████████
        ███    ███ ▀███████████ ███   ███ ███           ███
        ███    ███   ███    ███ ███   ███ ███     ▄█    ███
        ███    █▀    ███    ███  ▀█   █▀  █▀    ▄████████▀
                     ███    ███

                          版本 {}
                {}
        "#,
        version,
        repository.bright_white().bold()
    );
}

fn main() {
    // Parse arguments to decide whether to launch the UI or CLI
    let raw_args: Vec<String> = std::env::args().collect();

    // Check if either `--help` or `--path` is present to run command-line mode
    let is_help: bool = raw_args.iter().any(|arg: &String| arg == "--help");
    let is_path_provided: bool = raw_args
        .iter()
        .any(|arg: &String| arg.starts_with("--path"));

    if is_help || is_path_provided {
        print_banner();

        // Check for updates
        if let Err(e) = version_check::check_for_updates() {
            eprintln!(
                "{}: {}",
                "检查版本更新时出错".red().bold(),
                e
            );
        }

        // Parse input arguments
        let args: Args = Args::parse();
        args.run();

        let bbox: Vec<f64> = args
            .bbox
            .as_ref()
            .expect("需要边界框")
            .split(',')
            .map(|s: &str| s.parse::<f64>().expect("边界框坐标无效"))
            .collect::<Vec<f64>>();

        let bbox_tuple: (f64, f64, f64, f64) = (bbox[0], bbox[1], bbox[2], bbox[3]);

        // Fetch data
        let raw_data: serde_json::Value =
            retrieve_data::fetch_data(bbox_tuple, args.file.as_deref(), args.debug, "requests")
                .expect("无法获取数据");

        // Parse raw data
        let (mut parsed_elements, scale_factor_x, scale_factor_z) =
            osm_parser::parse_osm_data(&raw_data, bbox_tuple, &args);
        parsed_elements.sort_by_key(|element: &osm_parser::ProcessedElement| {
            osm_parser::get_priority(element)
        });

        // Write the parsed OSM data to a file for inspection
        if args.debug {
            let mut output_file: File =
                File::create("parsed_osm_data.txt").expect("无法创建输出文件");
            for element in &parsed_elements {
                writeln!(
                    output_file,
                    "元素 ID：{}，类型：{}，标签：{:?}",
                    element.id(),
                    element.kind(),
                    element.tags(),
                )
                .expect("无法写入输出文件");
            }
        }

        // Generate world
        let _ =
            data_processing::generate_world(parsed_elements, &args, scale_factor_x, scale_factor_z);
    } else {
        // Launch the UI
        println!("正在启动 UI...");
        tauri::Builder::default()
            .invoke_handler(tauri::generate_handler![
                gui_select_world,
                gui_start_generation,
                gui_get_version,
                gui_check_for_updates
            ])
            .setup(|app| {
                let app_handle = app.handle();
                let main_window = tauri::Manager::get_webview_window(app_handle, "main")
                    .expect("无法获取主窗口");
                progress::set_main_window(main_window);
                Ok(())
            })
            .run(tauri::generate_context!())
            .expect("启动应用程序 UI (Tauri) 时出错");
    }
}

#[tauri::command]
fn gui_select_world(generate_new: bool) -> Result<String, String> {
    // Determine the default Minecraft 'saves' directory based on the OS
    let default_dir: Option<PathBuf> = if cfg!(target_os = "windows") {
        env::var("APPDATA")
            .ok()
            .map(|appdata: String| PathBuf::from(appdata).join(".minecraft").join("saves"))
    } else if cfg!(target_os = "macos") {
        dirs::home_dir().map(|home: PathBuf| {
            home.join("Library/Application Support/minecraft")
                .join("saves")
        })
    } else if cfg!(target_os = "linux") {
        dirs::home_dir().map(|home: PathBuf| home.join(".minecraft").join("saves"))
    } else {
        None
    };

    if generate_new {
        // Handle new world generation
        if let Some(default_path) = &default_dir {
            if default_path.exists() {
                // Generate a unique world name
                let mut counter: i32 = 1;
                let unique_name: String = loop {
                    let candidate_name: String = format!("Arnis的世界 {}", counter);
                    let candidate_path: PathBuf = default_path.join(&candidate_name);
                    if !candidate_path.exists() {
                        break candidate_name;
                    }
                    counter += 1;
                };

                let new_world_path: PathBuf = default_path.join(&unique_name);

                // Create the new world structure
                create_new_world(&new_world_path, &unique_name)?;
                Ok(new_world_path.display().to_string())
            } else {
                Err("未找到 Minecraft 目录。".to_string())
            }
        } else {
            Err("未找到 Minecraft 目录。".to_string())
        }
    } else {
        // Handle existing world selection
        // Open the directory picker dialog
        let dialog: FileDialog = FileDialog::new();
        let dialog: FileDialog = if let Some(start_dir) = default_dir.filter(|dir| dir.exists()) {
            dialog.set_directory(start_dir)
        } else {
            dialog
        };

        if let Some(path) = dialog.pick_folder() {
            // Check if the "region" folder exists within the selected directory
            if path.join("region").exists() {
                // Check the 'session.lock' file
                let session_lock_path = path.join("session.lock");
                if session_lock_path.exists() {
                    // Try to acquire a lock on the session.lock file
                    if let Ok(file) = File::open(&session_lock_path) {
                        if file.try_lock_shared().is_err() {
                            return Err("所选世界目前正在使用中".to_string());
                        } else {
                            // Release the lock immediately
                            let _ = file.unlock();
                        }
                    }
                }

                return Ok(path.display().to_string());
            } else {
                // No Minecraft directory found, generating world in custom user selected directory

                // Generate a unique world name
                let mut counter: i32 = 1;
                let unique_name: String = loop {
                    let candidate_name: String = format!("Arnis的世界 {}", counter);
                    let candidate_path: PathBuf = path.join(&candidate_name);
                    if !candidate_path.exists() {
                        break candidate_name;
                    }
                    counter += 1;
                };

                let new_world_path: PathBuf = path.join(&unique_name);

                // Create the new world structure
                create_new_world(&new_world_path, &unique_name)?;
                return Ok(new_world_path.display().to_string());
            }
        }

        // If no folder was selected, return an error message
        Err("未选择世界".to_string())
    }
}

fn create_new_world(world_path: &Path, world_name: &str) -> Result<(), String> {
    // Create the new world directory structure
    fs::create_dir_all(world_path.join("region"))
        .map_err(|e: std::io::Error| format!("无法创建世界目录：{}", e))?;

    // Copy the region template file
    const REGION_TEMPLATE: &[u8] = include_bytes!("../mcassets/region.template");
    let region_path = world_path.join("region").join("r.0.0.mca");
    fs::write(&region_path, REGION_TEMPLATE)
        .map_err(|e: std::io::Error| format!("无法创建区域文件：{}", e))?;

    // Add the level.dat file
    const LEVEL_TEMPLATE: &[u8] = include_bytes!("../mcassets/level.dat");

    // Decompress the gzipped level.template
    let mut decoder: GzDecoder<&[u8]> = GzDecoder::new(LEVEL_TEMPLATE);
    let mut decompressed_data: Vec<u8> = Vec::new();
    decoder
        .read_to_end(&mut decompressed_data)
        .map_err(|e: std::io::Error| format!("无法解压 level.template: {}", e))?;

    // Parse the decompressed NBT data
    let mut level_data: Value = fastnbt::from_bytes(&decompressed_data)
        .map_err(|e: fastnbt::error::Error| format!("无法解析 level.dat 模板：{}", e))?;

    // Modify the LevelName and LastPlayed fields
    if let Value::Compound(ref mut root) = level_data {
        if let Some(Value::Compound(ref mut data)) = root.get_mut("Data") {
            // Update LevelName
            data.insert(
                "LevelName".to_string(),
                Value::String(world_name.to_string()),
            );

            // Update LastPlayed to the current Unix time in milliseconds
            let current_time: std::time::Duration = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e: std::time::SystemTimeError| {
                    format!("无法获取当前时间：{}", e)
                })?;
            let current_time_millis: i64 = current_time.as_millis() as i64;
            data.insert("LastPlayed".to_string(), Value::Long(current_time_millis));
        }
    }

    // Serialize the updated NBT data back to bytes
    let serialized_level_data: Vec<u8> =
        fastnbt::to_bytes(&level_data).map_err(|e: fastnbt::error::Error| {
            format!("无法序列化更新的 level.dat：{}", e)
        })?;

    // Compress the serialized data back to gzip
    let mut encoder: flate2::write::GzEncoder<Vec<u8>> =
        flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder
        .write_all(&serialized_level_data)
        .map_err(|e: std::io::Error| format!("无法压缩更新的 level.dat：{}", e))?;
    let compressed_level_data: Vec<u8> = encoder.finish().map_err(|e: std::io::Error| {
        format!("无法完成 level.dat 的压缩：{}", e)
    })?;

    fs::write(world_path.join("level.dat"), compressed_level_data)
        .map_err(|e: std::io::Error| format!("无法创建 level.dat 文件：{}", e))?;

    // Add the icon.png file
    const ICON_TEMPLATE: &[u8] = include_bytes!("../mcassets/icon.png");
    fs::write(world_path.join("icon.png"), ICON_TEMPLATE)
        .map_err(|e: std::io::Error| format!("无法创建 icon.png 文件：{}", e))?;

    Ok(())
}

#[tauri::command]
fn gui_get_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[tauri::command]
fn gui_check_for_updates() -> Result<bool, String> {
    match version_check::check_for_updates() {
        Ok(is_newer) => Ok(is_newer),
        Err(e) => Err(format!("检查更新时出错：{}", e)),
    }
}

#[tauri::command]
fn gui_start_generation(
    bbox_text: String,
    selected_world: String,
    world_scale: f64,
    ground_level: i32,
    winter_mode: bool,
    floodfill_timeout: u64,
) -> Result<(), String> {
    tauri::async_runtime::spawn(async move {
        if let Err(e) = tokio::task::spawn_blocking(move || {
            // Utility function to reorder bounding box coordinates
            fn reorder_bbox(bbox: &[f64]) -> (f64, f64, f64, f64) {
                (bbox[1], bbox[0], bbox[3], bbox[2])
            }

            // Parse bounding box string and validate it
            let bbox: Vec<f64> = bbox_text
                .split_whitespace()
                .map(|s| s.parse::<f64>().expect("边界框坐标无效"))
                .collect();

            if bbox.len() != 4 {
                return Err("边界框格式无效".to_string());
            }

            // Create an Args instance with the chosen bounding box and world directory path
            let args: Args = Args {
                bbox: Some(bbox_text),
                file: None,
                path: selected_world,
                downloader: "requests".to_string(),
                scale: world_scale,
                ground_level,
                winter: winter_mode,
                debug: false,
                timeout: Some(std::time::Duration::from_secs(floodfill_timeout)),
            };

            // Reorder bounding box coordinates for further processing
            let reordered_bbox: (f64, f64, f64, f64) = reorder_bbox(&bbox);

            // Run data fetch and world generation
            match retrieve_data::fetch_data(reordered_bbox, None, args.debug, "requests") {
                Ok(raw_data) => {
                    let (mut parsed_elements, scale_factor_x, scale_factor_z) =
                        osm_parser::parse_osm_data(&raw_data, reordered_bbox, &args);
                    parsed_elements.sort_by_key(|element: &osm_parser::ProcessedElement| {
                        osm_parser::get_priority(element)
                    });

                    let _ = data_processing::generate_world(
                        parsed_elements,
                        &args,
                        scale_factor_x,
                        scale_factor_z,
                    );
                    Ok(())
                }
                Err(e) => Err(format!("无法开始生成：{}", e)),
            }
        })
        .await
        {
            eprintln!("阻止任务时出错：{}", e);
        }
    });

    Ok(())
}
