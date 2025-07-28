use crate::core::config::DeviceSettings;
use crate::core::sync::{apply_pkg_state_commands, CorePackage, Phone, User};
use crate::core::utils::DisplayablePath;
use crate::gui::widgets::package_row::PackageRow;
use crate::CACHE_DIR;
use serde::{Deserialize, Serialize};
use static_init::dynamic;
use std::fs::{self, DirEntry};
use std::path::{Path, PathBuf};

#[dynamic]
pub static BACKUP_DIR: PathBuf = CACHE_DIR.join("backups");

#[derive(Default, Deserialize, Serialize, Debug, Clone, PartialEq, Eq)]
struct PhoneBackup {
    device_id: String,
    users: Vec<UserBackup>,
}

#[derive(Default, Deserialize, Serialize, Debug, Clone, PartialEq, Eq)]
struct UserBackup {
    id: u16,
    packages: Vec<CorePackage>,
}

// Backup all `Uninstalled` and `Disabled` packages
pub async fn backup_phone(
    users: &[User],
    device_id: &str,
    phone_packages: &[Vec<PackageRow>],
) -> Result<(), String> {
    let backup = users.iter().enumerate().fold(
        PhoneBackup {
            device_id: device_id.to_string(),
            ..Default::default()
        },
        |mut acc, (index, user)| {
            let user_backup = UserBackup {
                id: user.id,
                packages: phone_packages[index]
                    .iter()
                    .map(|p| CorePackage {
                        name: p.name.clone(),
                        state: p.state,
                    })
                    .collect(),
                ..Default::default()
            };
            acc.users.push(user_backup);
            acc
        },
    );

    let backup_path = BACKUP_DIR.join(device_id);
    if let Err(e) = fs::create_dir_all(&backup_path) {
        error!("BACKUP: could not create backup dir: {}", e);
        return Err(e.to_string());
    }

    let backup_filename = format!("{}.json", chrono::Local::now().format("%Y-%m-%d_%H-%M-%S"));
    let json = serde_json::to_string_pretty(&backup).map_err(|e| e.to_string())?;
    fs::write(backup_path.join(backup_filename), json).map_err(|e| e.to_string())?;
    
    Ok(())
}

pub fn list_available_backups(dir: &Path) -> Vec<DisplayablePath> {
    fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry: DirEntry| DisplayablePath { path: entry.path() })
                .collect()
        })
        .unwrap_or_default()
}

pub fn list_available_backup_users(backup: &DisplayablePath) -> Vec<User> {
    match fs::read_to_string(&backup.path) {
        Ok(data) => match serde_json::from_str::<PhoneBackup>(&data) {
            Ok(phone_backup) => phone_backup
                .users
                .into_iter()
                .map(|u| User {
                    id: u.id,
                    index: 0,
                    protected: false,
                })
                .collect(),
            Err(e) => {
                error!("[BACKUP]: Failed to parse backup file: {}", e);
                vec![]
            }
        },
        Err(e) => {
            error!("[BACKUP]: Selected backup file not found: {}", e);
            vec![]
        }
    }
}

#[derive(Debug)]
pub struct BackupPackage {
    pub index: usize,
    pub commands: Vec<String>,
}

pub fn restore_backup(
    selected_device: &Phone,
    packages: &[Vec<PackageRow>],
    settings: &DeviceSettings,
) -> Result<Vec<BackupPackage>, String> {
    let backup_path = settings
        .backup
        .selected
        .as_ref()
        .ok_or("No backup selected")?
        .path
        .clone();

    let data = fs::read_to_string(&backup_path).map_err(|e| e.to_string())?;
    let phone_backup: PhoneBackup = serde_json::from_str(&data).map_err(|e| e.to_string())?;

    let mut commands = Vec::new();
    let selected_user = settings
        .backup
        .selected_user
        .as_ref()
        .ok_or("No user selected")?;

    for user_backup in phone_backup.users {
        let user_index = selected_device
            .user_list
            .iter()
            .find(|x| x.id == user_backup.id)
            .ok_or_else(|| format!("User {} doesn't exist", user_backup.id))?
            .index;

        for (i, backup_package) in user_backup.packages.iter().enumerate() {
            let package = packages[user_index]
                .iter()
                .find(|x| x.name == backup_package.name)
                .map(|p| p.into())
                .ok_or_else(|| format!("Package {} not found for user {}", backup_package.name, user_backup.id))?;

            let p_commands = apply_pkg_state_commands(
                &package,
                backup_package.state,
                selected_user,
                selected_device,
            );

            if !p_commands.is_empty() {
                commands.push(BackupPackage {
                    index: i,
                    commands: p_commands,
                });
            }
        }
    }

    if !commands.is_empty() {
        commands.push(BackupPackage {
            index: 0,
            commands: Vec::new(),
        });
    }

    Ok(commands)
}
