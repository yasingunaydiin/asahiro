// stands in for steam's steamwebhelper.exe in the game bottle and starts valve's binary, renamed to
// steamwebhelper_real.exe, with two extra chromium flags. without them cef 126 paints every steam window
// black under wine on macos: --disable-gpu keeps rasterising on the cpu, --single-process keeps the gpu,
// renderer and network services inside one process so nothing has to cross a process boundary in wine.
// steam ignores its own -cef-single-process, so the flags have to be added here. core embeds the copy that
// scripts/build.sh builds into app/Resources/bin
#include <windows.h>

#define REAL_NAME L"steamwebhelper_real.exe"
#define EXTRA_FLAGS L"--disable-gpu --single-process"

// skips argv[0] the way the windows command line parser does, quoted or not
static const wchar_t *skip_program(const wchar_t *p) {
    if (*p == L'"') {
        for (p++; *p && *p != L'"'; p++) {}
        if (*p) p++;
    } else {
        while (*p && *p != L' ' && *p != L'\t') p++;
    }
    while (*p == L' ' || *p == L'\t') p++;
    return p;
}

int WINAPI wWinMain(HINSTANCE inst, HINSTANCE prev, PWSTR cmd, int show) {
    (void)inst; (void)prev; (void)cmd; (void)show;
    wchar_t dir[MAX_PATH];
    DWORD n = GetModuleFileNameW(NULL, dir, MAX_PATH);
    if (n == 0 || n >= MAX_PATH) return 1;
    while (n > 0 && dir[n - 1] != L'\\') n--;
    dir[n] = 0;

    const wchar_t *args = skip_program(GetCommandLineW());
    size_t len = lstrlenW(dir) + lstrlenW(REAL_NAME) + lstrlenW(EXTRA_FLAGS) + lstrlenW(args) + 8;
    wchar_t *line = HeapAlloc(GetProcessHeap(), 0, len * sizeof(wchar_t));
    if (!line) return 1;
    // wsprintfW stops at 1024 characters and steam's command lines run longer
    lstrcpyW(line, L"\"");
    lstrcatW(line, dir);
    lstrcatW(line, REAL_NAME L"\" " EXTRA_FLAGS L" ");
    lstrcatW(line, args);

    STARTUPINFOW si = { .cb = sizeof(si) };
    PROCESS_INFORMATION pi;
    if (!CreateProcessW(NULL, line, NULL, NULL, FALSE, 0, NULL, dir, &si, &pi)) return 1;
    WaitForSingleObject(pi.hProcess, INFINITE);
    DWORD code = 1;
    GetExitCodeProcess(pi.hProcess, &code);
    CloseHandle(pi.hThread);
    CloseHandle(pi.hProcess);
    return (int)code;
}
