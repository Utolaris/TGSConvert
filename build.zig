const std = @import("std");

pub fn build(b: *std.Build) void {
    const zig_exe = b.graph.zig_exe;

    const win_x86 = b.step("win-x86", "Cross-compile tgs-webm.exe (Windows x86) with zig");
    b.default_step = win_x86;

    const toolchain = make_win32_toolchain(zig_exe) catch |err| {
        std.debug.print("error: failed to prepare zig win32 toolchain: {s}\n", .{@errorName(err)});
        std.process.exit(1);
    };

    const rustup = b.addSystemCommand(&.{ "rustup", "target", "add", "i686-pc-windows-gnu" });

    const cargo = b.addSystemCommand(&.{
        "cargo", "build", "--target", "i686-pc-windows-gnu", "--release",
    });
    cargo.setCwd(b.path("tgs-webm"));
    cargo.setEnvironmentVariable("CC_i686_pc_windows_gnu", b.pathJoin(&.{ toolchain, "cc.sh" }));
    cargo.setEnvironmentVariable("CXX_i686_pc_windows_gnu", b.pathJoin(&.{ toolchain, "cxx.sh" }));
    cargo.setEnvironmentVariable("AR_i686_pc_windows_gnu", b.pathJoin(&.{ toolchain, "ar.sh" }));
    cargo.setEnvironmentVariable("RANLIB_i686_pc_windows_gnu", b.pathJoin(&.{ toolchain, "ranlib.sh" }));
    cargo.setEnvironmentVariable(
        "CARGO_TARGET_I686_PC_WINDOWS_GNU_LINKER",
        b.pathJoin(&.{ toolchain, "linker.sh" }),
    );
    if (std.posix.getenv("PATH")) |path| {
        cargo.setEnvironmentVariable("PATH", b.fmt("{s}:{s}", .{ toolchain, path }));
    }
    cargo.step.dependOn(&rustup.step);
    win_x86.dependOn(&cargo.step);

    const install = b.addInstallFileWithDir(
        b.path("tgs-webm/target/i686-pc-windows-gnu/release/tgs-webm.exe"),
        .bin,
        "tgs-webm.exe",
    );
    install.step.dependOn(&cargo.step);
    b.getInstallStep().dependOn(&install.step);

    const mac = b.step("mac", "Build the native macOS binary");
    const cargo_mac = b.addSystemCommand(&.{ "cargo", "build", "--release" });
    cargo_mac.setCwd(b.path("tgs-webm"));
    mac.dependOn(&cargo_mac.step);
    const install_mac = b.addInstallFileWithDir(
        b.path("tgs-webm/target/release/tgs-webm"),
        .bin,
        "tgs-webm",
    );
    install_mac.step.dependOn(&cargo_mac.step);
    mac.dependOn(&install_mac.step);

    const test = b.step("test", "Run cargo tests on the host");
    const cargo_test = b.addSystemCommand(&.{ "cargo", "test" });
    cargo_test.setCwd(b.path("tgs-webm"));
    test.dependOn(&cargo_test.step);
}

fn make_win32_toolchain(zig_exe: []const u8) ![]const u8 {
    const alloc = std.heap.page_allocator;
    const base = std.posix.getenv("TMPDIR") orelse "/tmp";
    const dir = try std.fs.path.join(alloc, &.{ base, "tgsconvert-zig-win32" });
    std.fs.makeDirAbsolute(dir) catch |err| switch (err) {
        error.PathAlreadyExists => {},
        else => return err,
    };

    const cc = try std.fmt.allocPrint(alloc,
        \\#!/bin/sh
        \\args=""
        \\for arg in "$@"; do
        \\  case "$arg" in
        \\    --target=i686-pc-windows-gnu|-m32) continue ;;
        \\  esac
        \\  args="$args $(printf '%s' "$arg" | sed "s/['\"\\]/\\\\&/g")"
        \\done
        \\eval "exec '{s}' cc -target x86-windows-gnu $args"
        \\
    , .{zig_exe});
    const cxx = try std.fmt.allocPrint(alloc,
        \\#!/bin/sh
        \\args=""
        \\for arg in "$@"; do
        \\  case "$arg" in
        \\    --target=i686-pc-windows-gnu|-m32) continue ;;
        \\  esac
        \\  args="$args $(printf '%s' "$arg" | sed "s/['\"\\]/\\\\&/g")"
        \\done
        \\eval "exec '{s}' c++ -target x86-windows-gnu $args"
        \\
    , .{zig_exe});
    const linker = try std.fmt.allocPrint(alloc,
        \\#!/bin/sh
        \\args=""
        \\for arg in "$@"; do
        \\  case "$arg" in
        \\    --target=i686-pc-windows-gnu|-m32) continue ;;
        \\  esac
        \\  args="$args $(printf '%s' "$arg" | sed "s/['\"\\]/\\\\&/g")"
        \\done
        \\eval "exec '{s}' cc -target x86-windows-gnu $args"
        \\
    , .{zig_exe});
    const ar = try std.fmt.allocPrint(alloc,
        \\#!/bin/sh
        \\if [ -n "$LLVM_AR" ] && [ -x "$LLVM_AR" ]; then exec "$LLVM_AR" "$@"; fi
        \\for p in /opt/homebrew/opt/llvm*/bin/llvm-ar /usr/local/opt/llvm*/bin/llvm-ar; do
        \\  if [ -x "$p" ]; then exec "$p" "$@"; fi
        \\done
        \\if command -v i686-w64-mingw32-ar >/dev/null 2>&1; then exec i686-w64-mingw32-ar "$@"; fi
        \\exec '{s}' ar "$@"
        \\
    , .{zig_exe});
    const ranlib = try std.fmt.allocPrint(alloc,
        \\#!/bin/sh
        \\exec '{s}' ranlib "$@"
        \\
    , .{zig_exe});
    const dlltool = try std.fmt.allocPrint(alloc,
        \\#!/bin/sh
        \\if command -v i686-w64-mingw32-dlltool >/dev/null 2>&1; then exec i686-w64-mingw32-dlltool "$@"; fi
        \\exec '{s}' dlltool "$@"
        \\
    , .{zig_exe});
    const cmake =
        \\#!/bin/sh
        \\if [ "$1" = "--build" ]; then
        \\  exec $(command -v cmake) "$@"
        \\fi
        \\here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
        \\stripped=""
        \\IFS=:
        \\for entry in $PATH; do
        \\  if [ "$entry" = "$here" ]; then continue; fi
        \\  if [ -n "$stripped" ]; then stripped="$stripped:"; fi
        \\  stripped="$stripped$entry"
        \\done
        \\PATH="$stripped"
        \\export PATH
        \\real_cmake=$(command -v cmake)
        \\exec "$real_cmake" \
        \\  -DCMAKE_SYSTEM_NAME=Windows \
        \\  -DCMAKE_SYSTEM_PROCESSOR=x86 \
        \\  -DCMAKE_C_COMPILER="$here/cc.sh" \
        \\  -DCMAKE_CXX_COMPILER="$here/cxx.sh" \
        \\  -DCMAKE_AR="$here/ar.sh" \
        \\  -DCMAKE_RANLIB="$here/ranlib.sh" \
        \\  "$@"
        \\
    ;

    try write_script(dir, "cc.sh", cc);
    try write_script(dir, "cxx.sh", cxx);
    try write_script(dir, "linker.sh", linker);
    try write_script(dir, "ar.sh", ar);
    try write_script(dir, "ranlib.sh", ranlib);
    try write_script(dir, "i686-w64-mingw32-dlltool", dlltool);
    try write_script(dir, "cmake", cmake);
    return dir;
}

fn write_script(dir: []const u8, name: []const u8, content: []const u8) !void {
    const path = try std.fs.path.join(std.heap.page_allocator, &.{ dir, name });
    const file = try std.fs.cwd().createFile(path, .{ .mode = 0o755 });
    defer file.close();
    try file.writeAll(content);
}
