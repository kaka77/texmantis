/*
 * TexMantis Win7 / Server 2008 R2 兼容垫片 —— bcryptprimitives.dll
 *
 * 背景：Rust 1.78+ 标准库的随机源改为 ProcessPrng（位于 Win8+ 的系统
 * bcryptprimitives.dll）。Win7/2008R2 没有该 DLL，加载器直接报"缺少
 * bcryptprimitives.dll"。本垫片导出同名同签名的 ProcessPrng，
 * 转发到 XP+ 一直存在的 RtlGenRandom（advapi32!SystemFunction036）。
 *
 * 安全性：bcryptprimitives.dll 仅被主程序导入 ProcessPrng 一个符号，
 * 因此在 Win8+ 系统上即使本文件被优先加载也不会缺失任何导出；
 * 随机性由操作系统原生 RtlGenRandom 提供。
 */
#include <windows.h>

BOOL __stdcall ProcessPrng(unsigned char *pbData, size_t cbData)
{
    HMODULE h;
    FARPROC f;
    if (cbData > 0xFFFFFFFFUL) { SetLastError(ERROR_INVALID_PARAMETER); return FALSE; }
    h = GetModuleHandleW(L"advapi32.dll");
    if (!h) h = LoadLibraryW(L"advapi32.dll");
    if (!h) return FALSE;
    f = GetProcAddress(h, "SystemFunction036"); /* RtlGenRandom */
    if (!f) return FALSE;
    return ((BOOL (WINAPI *)(PVOID, ULONG))(void *)f)(pbData, (ULONG)cbData);
}
