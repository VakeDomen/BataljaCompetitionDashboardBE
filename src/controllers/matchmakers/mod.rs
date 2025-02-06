use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

use crate::models::errors::MatchMakerError;

pub mod capped;
pub mod classic2v2;
pub mod pairmakers;

/// Cleans up the matches directory by removing all sub-directories.
///
/// This function is designed to remove all game-related folders that were
/// created during individual matches within the `./resources/matches/` directory.
/// It ensures the top-level `matches` directory remains intact while all its
/// sub-directories (representing individual matches) are deleted.
///
/// # Returns
///
/// A `Result` which is `Ok(())` if the cleanup was successful, or a `MatchMakerError`
/// if there's an error during the cleanup process.
///
pub fn cleanup_matches() -> Result<(), MatchMakerError> {
    // Cleanup: Remove all sub-directories within the ./resources/matches/ directory
    let matches_path = Path::new("./resources/matches");
    if let Ok(entries) = fs::read_dir(matches_path) {
        for entry in entries {
            if let Ok(entry) = entry {
                if entry.path().is_dir() {
                    if let Err(e) = fs::remove_dir_all(entry.path()) {
                        return Err(MatchMakerError::IOError(e));
                    }
                }
            }
        }
    }

    if let Err(e) = kill_java_player_processes() {
        eprintln!("Failed killing java processes: {:?}", e);
    }
    Ok(())
}

/// Kill all processes running with the command "java Player."
pub fn kill_java_player_processes() -> Result<(), std::io::Error> {
    // Get a list of all processes with "java Player" in their command line
    let ps_output = Command::new("ps").arg("ax").output()?;

    // Convert the output to a string
    let ps_output_str = String::from_utf8_lossy(&ps_output.stdout);

    // Split the output into lines
    let process_lines: Vec<&str> = ps_output_str.lines().collect();

    // Iterate through the lines and find processes with "java Player"
    for process_line in process_lines {
        if process_line.contains("java Player") {
            // Extract the process ID (PID)
            let pid_str = process_line.split_whitespace().next().unwrap_or_default();

            // Parse the PID as an integer
            if let Ok(pid) = pid_str.parse::<i32>() {
                // Kill the process using the "kill" command
                let kill_result = Command::new("kill")
                    .arg("-9") // Use SIGKILL to forcefully terminate the process
                    .arg(pid.to_string())
                    .output();

                match kill_result {
                    Ok(Output {
                        status,
                        stdout,
                        stderr,
                    }) => {
                        if status.success() {
                            println!(
                                "Killed process with PID {}: {:?}",
                                pid,
                                String::from_utf8_lossy(&stdout)
                            );
                        } else {
                            eprintln!(
                                "Failed to kill process with PID {}: {:?}",
                                pid,
                                String::from_utf8_lossy(&stderr)
                            );
                        }
                    }
                    Err(e) => {
                        eprintln!("Error killing process with PID {}: {:?}", pid, e);
                    }
                }
            }
        }
    }

    Ok(())
}
