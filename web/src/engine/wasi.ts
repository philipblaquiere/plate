/**
 * Just enough of WASI for the configuration engine. It's built for `wasm32-wasip1` so Nickel
 * can resolve imports (it needs absolute paths, which only WASI has), but it never touches real
 * files: imported files are passed in with the one being evaluated. So there are no files, no
 * environment and no exit; only randomness (hash maps) and stderr (panic messages) do anything.
 */

const EBADF = 8;
const ENOSYS = 52;

export function wasiStubs(memory: () => WebAssembly.Memory): WebAssembly.Imports {
  const view = () => new DataView(memory().buffer);
  const decoder = new TextDecoder();
  let stderr = "";
  return {
    wasi_snapshot_preview1: {
      random_get(ptr: number, len: number): number {
        // getRandomValues takes at most 64 KiB at a time.
        for (let at = 0; at < len; at += 65536) {
          crypto.getRandomValues(new Uint8Array(memory().buffer, ptr + at, Math.min(65536, len - at)));
        }
        return 0;
      },
      environ_sizes_get(count: number, size: number): number {
        view().setUint32(count, 0, true);
        view().setUint32(size, 0, true);
        return 0;
      },
      environ_get: () => 0,
      fd_write(fd: number, iovs: number, n: number, written: number): number {
        const d = view();
        let total = 0;
        for (let i = 0; i < n; i++) {
          const ptr = d.getUint32(iovs + i * 8, true);
          const len = d.getUint32(iovs + i * 8 + 4, true);
          if (fd === 2) stderr += decoder.decode(new Uint8Array(memory().buffer, ptr, len));
          total += len;
        }
        d.setUint32(written, total, true);
        // Print whole lines (a panic message arrives in pieces).
        const lines = stderr.split("\n");
        stderr = lines.pop() ?? "";
        for (const line of lines) if (line) console.warn(`[engine] ${line}`);
        return 0;
      },
      // No preopened folders: every path lookup fails, as there are no files.
      fd_prestat_get: () => EBADF,
      fd_prestat_dir_name: () => EBADF,
      fd_close: () => EBADF,
      fd_fdstat_get: () => EBADF,
      fd_filestat_get: () => EBADF,
      fd_read: () => EBADF,
      path_filestat_get: () => ENOSYS,
      path_open: () => ENOSYS,
      proc_exit(code: number): never {
        throw new Error(`the engine exited (${code})`);
      },
    },
  };
}
