// windows side helper that runs inside the bottle and injects 3dmigoto the way xxmi launcher does.
// hook mode: a global hook from 3dmloader.dll loads d3d11.dll into the target process when it starts.
// direct mode, which xxmi uses for wuthering waves: waits for the target process and has 3dmloader.dll
// load d3d11.dll into it (its Inject export), d3dx.ini's dll_initialization_delay then holds 3dmigoto back.
// the game must not be a child of this process (the anti-cheat rejects that), so the launcher starts
// the game separately in the same wineserver while this waits.
// usage: asahiro-inject.exe <3dmloader.dll> <d3d11.dll> <process.exe> <timeout seconds> [direct]
// status lines go to stdout: hooked, waiting, injected, inject_timeout, error <what>
#include <windows.h>
#include <tlhelp32.h>
#include <stdio.h>
#include <wchar.h>

typedef int(__cdecl *hook_fn)(LPCWSTR, HHOOK *, HANDLE *);
typedef int(__cdecl *wait_fn)(LPCWSTR, LPCWSTR, int);
typedef int(__cdecl *unhook_fn)(HHOOK *, HANDLE *);
typedef int(__cdecl *inject_fn)(DWORD, LPCWSTR, int);

static void say(const char *fmt, ...) {
    va_list args;
    va_start(args, fmt);
    vprintf(fmt, args);
    va_end(args);
    printf("\n");
    fflush(stdout);
}

static DWORD find_process(const wchar_t *name) {
    DWORD pid = 0;
    HANDLE snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
    if (snap == INVALID_HANDLE_VALUE) return 0;
    PROCESSENTRY32W e = {.dwSize = sizeof(e)};
    for (BOOL more = Process32FirstW(snap, &e); more; more = Process32NextW(snap, &e)) {
        if (_wcsicmp(e.szExeFile, name) == 0) {
            pid = e.th32ProcessID;
            break;
        }
    }
    CloseHandle(snap);
    return pid;
}

static int run_direct(HMODULE loader, const wchar_t *dll_path, const wchar_t *process, int timeout) {
    inject_fn inject = (inject_fn)GetProcAddress(loader, "Inject");
    if (!inject) {
        say("error loader_exports");
        return 3;
    }
    say("waiting");
    DWORD start = GetTickCount();
    DWORD pid = 0;
    while (!(pid = find_process(process))) {
        if (GetTickCount() - start >= (DWORD)timeout * 1000) {
            say("inject_timeout");
            return 1;
        }
        Sleep(100);
    }
    int rc = inject(pid, dll_path, timeout);
    if (rc != 0) {
        say("error inject %d", rc);
        return 4;
    }
    say("injected");
    return 0;
}

int wmain(int argc, wchar_t **argv) {
    if (argc < 5) {
        say("error usage");
        return 2;
    }
    const wchar_t *loader_path = argv[1];
    const wchar_t *dll_path = argv[2];
    const wchar_t *process = argv[3];
    int timeout = _wtoi(argv[4]);

    HMODULE loader = LoadLibraryW(loader_path);
    if (!loader) {
        say("error load_loader %lu", GetLastError());
        return 3;
    }
    if (argc > 5 && _wcsicmp(argv[5], L"direct") == 0) {
        return run_direct(loader, dll_path, process, timeout);
    }
    hook_fn hook_library = (hook_fn)GetProcAddress(loader, "HookLibrary");
    wait_fn wait_for_injection = (wait_fn)GetProcAddress(loader, "WaitForInjection");
    unhook_fn unhook_library = (unhook_fn)GetProcAddress(loader, "UnhookLibrary");
    if (!hook_library || !wait_for_injection || !unhook_library) {
        say("error loader_exports");
        return 3;
    }

    HHOOK hook = NULL;
    HANDLE mutex = NULL;
    int rc = hook_library(dll_path, &hook, &mutex);
    if (rc != 0) {
        say("error hook %d", rc);
        return 4;
    }
    say("hooked");

    // the hook only fires while this process is alive, so keep it until the dll shows up in the game
    int ok = wait_for_injection(dll_path, process, timeout) == 0;
    say(ok ? "injected" : "inject_timeout");
    unhook_library(&hook, &mutex);
    return ok ? 0 : 1;
}
