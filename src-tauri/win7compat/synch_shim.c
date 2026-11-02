/*
 * TexMantis Win7 / Server 2008 R2 兼容垫片 —— api-ms-win-core-synch-l1-2-0.dll
 *
 * 背景：Rust 1.67+ 标准库的锁（futex 语义）使用 Win8+ 才有的 API 集
 * api-ms-win-core-synch-l1-2-0（WaitOnAddress / WakeByAddress*）。
 * Win7/2008R2 没有该 API 集，加载器按普通文件名解析 —— 因此应用目录下的
 * 本垫片会被加载；Win8+ 系统对 API 集名做系统级解析（优先于应用目录），
 * 本文件不会被加载，对现代系统零影响。
 *
 * 实现：按等待地址组织的等待队列，使用 Vista+ 即有的 SRWLock + 条件变量；
 * 语义对齐 MS 文档（值未变化即等待；Wake 摘链后唤醒；支持超时）。
 */
#include <windows.h>
#include <string.h>

typedef struct Waiter {
    struct Waiter *next;
    void *addr;
    CONDITION_VARIABLE cv;
} Waiter;

static SRWLOCK g_lock = SRWLOCK_INIT;
static Waiter *g_list = NULL;

__declspec(dllexport) BOOL __stdcall
WaitOnAddress(volatile VOID *Address, VOID *CompareAddress, SIZE_T AddressSize, DWORD dwMilliseconds)
{
    Waiter w;
    Waiter **pp;
    BOOL ok;
    if (AddressSize < 1 || AddressSize > 8) { SetLastError(ERROR_INVALID_PARAMETER); return FALSE; }
    w.addr = (void *)Address;
    InitializeConditionVariable(&w.cv);
    AcquireSRWLockExclusive(&g_lock);
    if (memcmp((const void *)Address, CompareAddress, AddressSize) != 0) {
        /* 值已变化：立即成功（对齐原生语义） */
        ReleaseSRWLockExclusive(&g_lock);
        return TRUE;
    }
    w.next = g_list;
    g_list = &w;
    ok = SleepConditionVariableSRW(&w.cv, &g_lock, dwMilliseconds, 0);
    /* 摘除自己（若已被 Wake 摘除则扫不到，安全） */
    pp = &g_list;
    while (*pp) {
        if (*pp == &w) { *pp = w.next; break; }
        pp = &(*pp)->next;
    }
    ReleaseSRWLockExclusive(&g_lock);
    return ok;
}

__declspec(dllexport) VOID __stdcall
WakeByAddressSingle(void *Address)
{
    Waiter **pp;
    AcquireSRWLockExclusive(&g_lock);
    for (pp = &g_list; *pp; pp = &(*pp)->next) {
        if ((*pp)->addr == Address) {
            WakeConditionVariable(&(*pp)->cv);
            *pp = (*pp)->next; /* 节点在等待者栈上，摘链后唤醒 */
            break;
        }
    }
    ReleaseSRWLockExclusive(&g_lock);
}

__declspec(dllexport) VOID __stdcall
WakeByAddressAll(void *Address)
{
    Waiter **pp;
    AcquireSRWLockExclusive(&g_lock);
    pp = &g_list;
    while (*pp) {
        if ((*pp)->addr == Address) {
            WakeConditionVariable(&(*pp)->cv);
            *pp = (*pp)->next;
        } else {
            pp = &(*pp)->next;
        }
    }
    ReleaseSRWLockExclusive(&g_lock);
}
