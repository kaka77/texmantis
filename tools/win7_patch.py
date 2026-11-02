# 对主程序做 Win7/Server 2008 R2 导入表兼容处理（就地改名，零依赖）：
#  1) kernel32!GetSystemTimePreciseAsFileTime（Win8+） -> GetSystemTimeAsFileTime（XP+，同签名，
#     Win7 上精度降为 ~15.6ms，对消息时间戳等场景足够）
#  2) user32!SetProcessDpiAwarenessContext（Win10 1703+） -> SetProcessDPIAware（Vista+，同为
#     1 参 BOOL；传入的上下文参数被忽略，Win7 上获得系统级 DPI 感知）
# 两处均为 KnownDLL（kernel32/user32），无法用本地 DLL 覆盖，只能改导入表名称。
# 精确定位：解析 PE 节表，只替换"导入表所在节"内的名字（导入名变短用 NUL 补齐；
# 其它节里的同名纯字符串字面量一律不动）；Hint 字失配时加载器按名称回退解析（PE 标准行为）。
import struct
import sys


def import_section(b: bytearray):
    """返回导入表所在节的 (name, raw_offset, raw_size, virtual_address)。"""
    e_lfanew = struct.unpack_from('<I', b, 0x3C)[0]
    assert b[e_lfanew:e_lfanew + 4] == b'PE\0\0', 'not a PE'
    coff = e_lfanew + 4
    nsec = struct.unpack_from('<H', b, coff + 2)[0]
    opt_size = struct.unpack_from('<H', b, coff + 16)[0]
    opt = coff + 20
    magic = struct.unpack_from('<H', b, opt)[0]
    ddir = opt + (96 if magic == 0x10B else 112)  # PE32 / PE32+ 数据目录偏移
    imp_rva = struct.unpack_from('<I', b, ddir + 8)[0]  # 目录[1] = 导入表
    sec = coff + 20 + opt_size
    for i in range(nsec):
        o = sec + 40 * i
        name = bytes(b[o:o + 8]).rstrip(b'\0').decode(errors='replace')
        vsize, va, rsize, roff = struct.unpack_from('<IIII', b, o + 8)
        if va <= imp_rva < va + max(vsize, rsize):
            return name, roff, rsize, va
    raise AssertionError('import directory section not found')


def patch(path: str) -> None:
    with open(path, 'rb') as f:
        b = bytearray(f.read())
    isec_name, roff, rsize, _va = import_section(b)
    in_isec = lambda i: roff <= i < roff + rsize  # noqa: E731

    def swap(old: str, new: str) -> None:
        # v0.9.27 幂等：已是目标名（旧名不在导入节）则跳过，重复运行安全
        ob = old.encode() + b'\0'
        nb = new.encode() + b'\0'
        hits = []
        i = b.find(ob)
        while i != -1:
            hits.append(i)
            i = b.find(ob, i + 1)
        targets = [i for i in hits if in_isec(i)]
        if not targets:
            alt = [i for i in hits_old(b, new) if in_isec(i)]
            assert alt, f'{old}/{new}: neither name in import section ({isec_name})'
            print(f'swap {old} -> {new}: already patched, skip')
            return
        assert len(targets) == 1, f'{old}: {len(targets)} occurrences in import section'
        pad = len(ob) - len(nb)
        assert pad >= 0, f'{new}: longer than {old}'
        t = targets[0]
        b[t:t + len(ob)] = nb + b'\0' * pad

    def hits_old(b: bytearray, name: str):
        needle = name.encode() + b'\0'
        out = []
        i = b.find(needle)
        while i != -1:
            out.append(i)
            i = b.find(needle, i + 1)
        return out

    swap('GetSystemTimePreciseAsFileTime', 'GetSystemTimeAsFileTime')
    swap('SetProcessDpiAwarenessContext', 'SetProcessDPIAware')
    scrub_build_paths(b)
    with open(path, 'wb') as f:
        f.write(bytes(b))
    print(f'win7-compat import patch ok (import section = {isec_name})')


# ---------------------------------------------------------------------------
# v0.9.27 构建环境路径擦除：panic 定位字符串（依赖 crate 的 registry 源码路径、
# std 的 /rustc/<hash>/ 路径）会把构建机目录编译进产物。等长清零填充 ——
# 不改变任何字节长度与地址（代码引用安全，无需重定位），panic 时仅显示空路径。
# 只擦"字符串起始处紧邻前一字节为 NUL"的出现（确为独立字符串字面量边界）。
# ---------------------------------------------------------------------------
import re as _re

_SCRUB = (
    _re.compile(rb"/var/dsh/buildstore/cargo/registry/src/[a-zA-Z0-9.\-]+/[a-zA-Z0-9_\-]+-[0-9][0-9a-zA-Z.\-]*/"),
    _re.compile(rb"/var/dsh/buildstore/cargo/git/[a-zA-Z0-9._\-]+/"),
    _re.compile(rb"/rustc/[0-9a-f]{40}/"),
)


def scrub_build_paths(b: bytearray) -> None:
    # v0.9.27b 放宽：Rust 常量池可能把 "Caused by:" 与路径串无 NUL 拼接，
    # 路径出现不再要求 NUL 边界。清零仅影响 panic 显示文案（display-only），
    # 不触及任何控制流/重定位，等长填充绝对安全。
    n = 0
    for pat in _SCRUB:
        for m in pat.finditer(bytes(b)):
            b[m.start():m.end()] = b"\x00" * (m.end() - m.start())
            n += 1
    print(f'build-path scrub ok ({n} location strings erased)')


if __name__ == '__main__':
    patch(sys.argv[1])
