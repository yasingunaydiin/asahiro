import Foundation

// the fork's identity, kept apart from upstream asahiro's so both can be installed side by side.
// the folder name matches APP_DIR in core/src/util.rs and the executable and bundle id match scripts/build.sh
enum AppInfo {
    static let name = "Asahiro Waves"
    static let bundleID = "com.yasingunaydin.asahiro-waves"
    static let executable = "AsahiroWaves"
    static let urlScheme = "asahiro-waves"
    static let gameName = "Wuthering Waves"
    static let repo = "yasingunaydiin/asahiro"

    static var supportDir: URL {
        FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Application Support/\(name)")
    }

    static var cachesDir: URL {
        FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Caches/\(name)")
    }
}
