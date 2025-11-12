// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use std::process::Command;
use std::path::{Path, PathBuf};
use std::fs;
use encoding_rs::GBK;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Debug, Serialize, Deserialize)]
struct UpxOptions {
    mode: String,
    input_file: String,
    output_file: String,
    compression_level: String,
    backup: bool,
    ultra_brute: bool,
    force: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct ScanFolderOptions {
    folder_path: String,
    include_subfolders: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct AppConfig {
    compression_level: i32,
    overwrite: bool,
    backup: bool,
    ultra_brute: bool,
    include_subfolders: bool,
    force_compress: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            compression_level: 9,
            overwrite: true,
            backup: false,
            ultra_brute: false,
            include_subfolders: false,
            force_compress: false,
        }
    }
}

// 获取配置文件路径
fn get_config_path() -> Option<PathBuf> {
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            return Some(exe_dir.join("upx_gui_config.json"));
        }
    }
    None
}

// 获取UPX可执行文件路径
fn get_upx_path() -> Option<PathBuf> {
    // 打包后的位置
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let upx = exe_dir.join("_up_/upx/upx.exe");
            if upx.exists() {
                return Some(upx);
            }
        }
    }

    // 开发环境
    let dev_upx = PathBuf::from("../upx/upx.exe");
    if dev_upx.exists() {
        return Some(dev_upx);
    }

    None
}

#[tauri::command]
async fn process_upx(options: UpxOptions) -> Result<String, String> {
    // 获取UPX可执行文件路径
    let upx_path = get_upx_path().ok_or("未找到 UPX 工具！请确保安装完整")?;
    
    // 检查 UPX 是否可用
    let mut upx_check = Command::new(&upx_path);
    
    // 在 Windows 上隐藏控制台窗口
    #[cfg(target_os = "windows")]
    upx_check.creation_flags(CREATE_NO_WINDOW);
    
    let upx_check = upx_check.arg("--version").output();
    
    if upx_check.is_err() {
        return Err("UPX 工具无法执行！".to_string());
    }

    // 检查输入文件是否存在
    if !Path::new(&options.input_file).exists() {
        return Err(format!("输入文件不存在: {}", options.input_file));
    }

    // 检查文件是否可写（如果需要覆盖）
    if options.input_file == options.output_file {
        let metadata = fs::metadata(&options.input_file)
            .map_err(|e| format!("无法读取文件属性: {}", e))?;
        
        if metadata.permissions().readonly() {
            return Err(format!("文件为只读，请先修改文件属性"));
        }
    }

    // 备份原文件
    if options.backup {
        let backup_path = format!("{}.bak", options.input_file);
        if let Err(e) = fs::copy(&options.input_file, &backup_path) {
            return Err(format!("备份文件失败: {}", e));
        }
    }

    // 构建 UPX 命令
    let mut cmd = Command::new(&upx_path);
    
    // 在 Windows 上隐藏控制台窗口
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);
    
    // 判断是否覆盖原文件
    let is_overwrite = options.input_file == options.output_file;
    
    match options.mode.as_str() {
        "compress" => {
            // 加壳模式
            if options.ultra_brute {
                cmd.arg("--ultra-brute");
                cmd.arg("--no-lzma"); // 禁用LZMA以避免解压速度过慢
            } else if options.compression_level == "best" {
                cmd.arg("--best");
            } else {
                cmd.arg(format!("-{}", options.compression_level));
            }
            
            // 添加强制压缩参数
            if options.force {
                cmd.arg("--force");
            }
            
            if is_overwrite {
                // 直接覆盖原文件，不使用 -o 参数
                cmd.arg(&options.input_file);
            } else {
                // 输出到指定文件
                cmd.arg(&options.input_file);
                cmd.arg("-o");
                cmd.arg(&options.output_file);
            }
            cmd.arg("--force-overwrite");
        }
        "decompress" => {
            // 脱壳模式
            cmd.arg("-d");
            
            // 添加强制压缩参数（脱壳时也可能需要）
            if options.force {
                cmd.arg("--force");
            }
            
            if is_overwrite {
                // 直接覆盖原文件
                cmd.arg(&options.input_file);
            } else {
                // 输出到指定文件
                cmd.arg(&options.input_file);
                cmd.arg("-o");
                cmd.arg(&options.output_file);
            }
            cmd.arg("--force-overwrite");
        }
        _ => {
            return Err("未知的操作模式".to_string());
        }
    }

    // 在处理前记录原始文件大小
    let original_size = fs::metadata(&options.input_file)
        .map(|m| m.len())
        .unwrap_or(0);
    
    // 在后台线程执行命令，避免阻塞 UI
    let output_file_clone = options.output_file.clone();
    let original_size_clone = original_size;
    
    tokio::task::spawn_blocking(move || {
        match cmd.output() {
            Ok(output) => {
                // 在 Windows 上 UPX 输出是 GBK 编码，需要转换为 UTF-8
                let (stdout, _, _) = GBK.decode(&output.stdout);
                let (stderr, _, _) = GBK.decode(&output.stderr);
                
                if output.status.success() {
                    // 获取处理后的文件大小
                    let output_size = fs::metadata(&output_file_clone)
                        .map(|m| m.len())
                        .unwrap_or(0);
                    
                    let ratio = if original_size_clone > 0 {
                        (output_size as f64 / original_size_clone as f64 * 100.0) as i32
                    } else {
                        100
                    };
                    
                    let result = format!(
                        "操作成功!\n输出: {}\n原始大小: {}\n处理后大小: {}\n压缩率: {}%\n\nUPX 输出:\n{}{}",
                        output_file_clone,
                        format_bytes(original_size_clone),
                        format_bytes(output_size),
                        ratio,
                        stdout,
                        stderr
                    );
                    Ok(result)
                } else {
                    Err(format!("UPX 执行失败:\n{}{}", stdout, stderr))
                }
            }
            Err(e) => {
                Err(format!("执行 UPX 命令失败: {}", e))
            }
        }
    })
    .await
    .map_err(|e| format!("任务执行错误: {}", e))?
}

// 格式化字节大小
fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} bytes", bytes)
    }
}

// 扫描文件夹获取所有exe和dll文件
fn scan_folder_recursive(folder_path: &Path, include_subfolders: bool) -> Vec<String> {
    let mut files = Vec::new();
    
    if let Ok(entries) = fs::read_dir(folder_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            
            if path.is_dir() && include_subfolders {
                // 递归扫描子文件夹
                let sub_files = scan_folder_recursive(&path, include_subfolders);
                files.extend(sub_files);
            } else if path.is_file() {
                if let Some(extension) = path.extension() {
                    let ext = extension.to_string_lossy().to_lowercase();
                    if ext == "exe" || ext == "dll" {
                        if let Some(path_str) = path.to_str() {
                            files.push(path_str.to_string());
                        }
                    }
                }
            }
        }
    }
    
    files
}

#[tauri::command]
fn scan_folder(options: ScanFolderOptions) -> Result<Vec<String>, String> {
    let path = Path::new(&options.folder_path);
    
    if !path.exists() {
        return Err(format!("路径不存在: {}", options.folder_path));
    }
    
    if !path.is_dir() {
        return Err(format!("不是文件夹: {}", options.folder_path));
    }
    
    Ok(scan_folder_recursive(path, options.include_subfolders))
}

// 获取UPX版本
#[tauri::command]
fn get_upx_version() -> Result<String, String> {
    let upx_path = get_upx_path().ok_or("未找到 UPX 工具！")?;
    
    let mut cmd = Command::new(&upx_path);
    
    // 在 Windows 上隐藏控制台窗口
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);
    
    match cmd.arg("--version").output() {
        Ok(output) => {
            let (version_str, _, _) = GBK.decode(&output.stdout);
            let lines: Vec<&str> = version_str.lines().collect();
            if !lines.is_empty() {
                Ok(lines[0].to_string())
            } else {
                Err("无法获取UPX版本".to_string())
            }
        }
        Err(_) => Err("UPX未找到".to_string())
    }
}

// 刷新图标缓存
#[tauri::command]
#[allow(dead_code)]
async fn refresh_icon_cache() -> Result<(), String> {
    // 后台执行刷新
    tokio::task::spawn_blocking(move || {
        #[cfg(target_os = "windows")]
        {
            // 1. 关闭 Explorer
            let _ = Command::new("taskkill")
                .args(&["/f", "/im", "explorer.exe"])
                .creation_flags(CREATE_NO_WINDOW)
                .output();

            std::thread::sleep(std::time::Duration::from_millis(500));

            // 2. 删除图标缓存
            if let Ok(userprofile) = std::env::var("USERPROFILE") {
                let cache_db = format!("{}\\AppData\\Local\\IconCache.db", userprofile);
                let _ = fs::remove_file(&cache_db);

                // 删除缩略图缓存
                let explorer_path = format!("{}\\AppData\\Local\\Microsoft\\Windows\\Explorer", userprofile);
                if let Ok(entries) = fs::read_dir(&explorer_path) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                            if name.starts_with("thumbcache_") {
                                let _ = fs::remove_file(&path);
                            }
                        }
                    }
                }
            }

            std::thread::sleep(std::time::Duration::from_millis(500));

            // 3. 重启 Explorer
            let _ = Command::new("explorer.exe")
                .creation_flags(CREATE_NO_WINDOW)
                .spawn();
        }
    });

    Ok(())
}

// 保存配置
#[tauri::command]
fn save_config(config: AppConfig) -> Result<(), String> {
    let config_path = get_config_path().ok_or("无法获取配置文件路径")?;
    let json = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("序列化配置失败: {}", e))?;
    fs::write(&config_path, json)
        .map_err(|e| format!("保存配置文件失败: {}", e))
}

// 加载配置
#[tauri::command]
fn load_config() -> Result<AppConfig, String> {
    let config_path = get_config_path().ok_or("无法获取配置文件路径")?;

    if !config_path.exists() {
        return Ok(AppConfig::default());
    }

    let json = fs::read_to_string(&config_path)
        .map_err(|e| format!("读取配置文件失败: {}", e))?;

    serde_json::from_str(&json)
        .map_err(|e| format!("解析配置文件失败: {}", e))
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            process_upx,
            scan_folder,
            get_upx_version,
            refresh_icon_cache,
            save_config,
            load_config
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
