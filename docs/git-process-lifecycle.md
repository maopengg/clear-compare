# Git 命令与辅助进程生命周期

所有工作台 Git 查询和写操作都通过同一执行器，使用参数数组，不拼接 shell 命令。保留原有本地 30 秒、网络 120 秒超时，以及 stdout 16 MiB、stderr 256 KiB 上限。

## 执行范围

macOS/Linux 为每条命令建立独立进程组。主进程退出后通过 `waitid(WNOWAIT)` 观察状态而不提前回收 PID；清理使用该独占进程组 ID，避免 PID 回收后复用引发误杀。执行器发出强制终止，并通过有界 `/bin/ps` 查询确认组内没有可运行成员，才回收主进程。查询只读取组 ID 和状态，不记录其他进程的命令、环境或用户信息。僵尸已经不能写入或创建后代，无须等待系统回收非直属后代；主进程由执行器回收。

Windows 建立设置了 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` 的 Job Object，先挂起启动主进程、加入 Job，再恢复其线程，避免加入之前创建后代的竞态。停止时终止 Job，并查询活动成员数归零。Job 的关闭策略还提供异常路径清理。

主进程正常或失败退出、用户取消、超时、输出超限以及监控错误都进行清理。主进程已经退出时保留其原始退出码；未完成的命令被强制终止。清理检查超过五秒或系统拒绝检查/终止时报告清理错误，不能将它表示为已成功取消。错误返回和 Rust unwind 路径提供析构清理；应用崩溃、强制退出、系统挂起不等于已完成清理。

## 边界与恢复

这是一条命令的生命周期约束，不是事务回滚或任意程序的沙箱。Unix hook 或第三方程序主动调用 setsid/setpgid 脱离进程组后，不在组内终止的保证范围。已经运行的外部凭据服务、用户 SSH 复用服务也不按仓库名称或进程名批量终止。Windows 的实际 Job 兼容性仍需对应系统运行验证。

取消可能保留已下载对象、已注册子模块、部分目录、锁文件或已完成步骤。不得自动删除未知锁文件或重试写操作；应刷新查看实际状态，再按 Git 诊断恢复。hook 刻意留在同组的后台任务会随命令一起清理，不能依赖它在客户端任务结束后持续执行。

## 验证

原生测试使用临时目录和运行时间有限的独立测试进程，验证运行中取消、三层进程、独立任务存活、正常/失败根进程的残留辅助进程清理、退出码、Drop/unwind、超时和两种输出上限，另验证快速成功退出不能绕过 stderr 上限。另用真实临时 Git 仓库的 pre-commit hook 验证取消返回后后台文件写入不再变化。macOS 本机通过；Windows 代码在 GNU 目标交叉编译和 strict Clippy 通过。Windows/Linux 实际运行结果另以对应提交 CI 为准。

操作系统 API 参考：[Rust Windows CommandExt](https://doc.rust-lang.org/std/os/windows/process/trait.CommandExt.html)、[Microsoft Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects)。
