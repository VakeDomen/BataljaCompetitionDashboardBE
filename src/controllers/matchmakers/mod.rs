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
