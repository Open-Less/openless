# Remote input: one-time phone certificate setup

Remote input needs HTTPS for browser microphone access. OpenLess generates a
private certificate authority (CA) on each computer and a separate server
certificate covering that computer's LAN addresses. The phone installs the
public CA certificate. The CA private key stays in the computer's user data.

## 首次信任前的一次性核验

根据 [#1037 的审核建议](https://github.com/Open-Less/openless/pull/1037#discussion_r3964815714)，
在信任根证书前，通过电脑本地设置与手机系统证书详情核对身份。

电脑设置中的 **本机根证书 SHA-256** 来自正在运行的监听器所用的公开根证书，
经本地接口传给界面；Linux 原生界面也显示同一来源的完整指纹。
指纹共 64 个十六进制字符，可忽略空格、冒号和大小写，但不能只核对开头几位。
对比对象必须是将要信任的根证书，不能使用会随 IP 变化而重新签发的服务器证书。

手机端必须从**系统证书详情**取得实际证书的 SHA-256。
网页、描述文件名称、标识和描述文字都可以被替换，不能作为校验依据。
描述文件名称末尾的短指纹仅用于区分电脑，不代表已经验证身份。
本功能提供人工核验依据，不会自动确认手机是否正确核对，也不会代替系统开启信任。

### iPhone 和 iPad

1. 在电脑启用远程输入，保留本地设置中的完整 SHA-256。指纹不可用时停止安装。
2. 在可信网络中，用 Safari 打开电脑显示的地址或复制的证书链接。
   首次 TLS 警告说明身份尚未验证；即使地址与电脑相同，也不能据此认定证书可信。
   只有准备执行下面的独立核验时，才继续下载描述文件；否则使用已有的可信文件传输渠道。
3. 在“设置 → 通用 → VPN 与设备管理”打开下载的描述文件。
   先检查它**只包含一张根证书**；若有额外证书、VPN 或设备管理配置，不要安装。
4. 在“更多详细信息”中打开根证书，查找系统显示的 SHA-256，
   与电脑本地设置中的全部 64 个字符逐一核对。
   若当前 iOS 只能在安装后显示完整详情，安装前仍需确认只有一张根证书，
   且安装后先保持“完全信任”关闭，核对完毕再开启。
5. 若指纹不一致、看不全或找不到 SHA-256，停止操作，删除下载的描述文件；
   已安装的则移除。不要用名称、网页上的值或配对码代替核验。
6. 只有全部一致，才到“通用 → 关于本机 → 证书信任设置”为这张根证书开启完全信任。
   返回 Safari 刷新，输入配对码并允许麦克风访问。

安装描述文件和开启完全信任是两个步骤，见[苹果说明](https://support.apple.com/en-us/102390)。
不同 iOS 版本的菜单和完整指纹入口需要真机确认；无法独立查看指纹的设备不能声称通过了本流程。
根证书可签发其他证书，私钥应始终保留在自己的电脑上。不再使用远程输入时请移除手机上的根证书。

### Android

通过“安卓：下载 CA 证书”取得 `/cert.cer`，在系统证书预览中核对完整 SHA-256，
一致后再安装。部分设备安装 CA 即代表信任，因此必须在安装前完成核验。
若系统无法在信任前显示完整指纹，请停止从网页安装，改用已有的可信文件传输渠道。
菜单名称和用户 CA 支持情况因设备而异。

### 真机验收与截图

自动化测试可验证指纹来源、替换证书时的差异及生命周期，不能代替以下真机操作：

| 检查 | 需要记录的结果 |
| --- | --- |
| 首次安装 | 电脑完整指纹、手机系统完整指纹一致；描述文件只有一张根证书 |
| 信任与录音 | 核对后开启完全信任，录音能正常传到电脑 |
| 重启 | 电脑重启后指纹不变，手机无需重新安装证书，仍可录音 |
| IP 变化 | 使用新地址重新连接后指纹不变，仍可录音 |
| 替换证书 | 在独立测试环境用另一台电脑生成的同名证书或替换描述文件，系统指纹不同，用户能按引导停止安装/信任 |

记录设备型号、系统版本、测试提交号，以及每项通过或失败。
截图至少包含电脑指纹、手机系统指纹和描述文件内容；录音、重启和 IP 变化可以用简短录屏或文字结果补充。
替换测试只核对差异，不要开启对测试证书的完全信任；结束后移除测试描述文件。
Linux 原生界面的显示与录音也需要在 Linux 上实际验收。

## What OpenLess automates

- Creates and atomically persists a unique CA and server identity on first use.
- Reuses the same CA and server certificate after restarts and upgrades that
  preserve application data.
- Reissues the server certificate when required LAN or virtual-adapter IPs
  change, keeping the same CA and therefore the phone's trust.
- Renews the server certificate on service startup when less than 30 days
  remain. Leaves are valid for at most 366 days including clock-skew allowance;
  a continuously running service must restart before its leaf expires.
- Serves the public CA as a `.cer` file or iOS configuration profile on both
  Tauri desktop and Linux egui hosts. The private keys are never downloaded.
- Refuses to start with a damaged, unreadable, mismatched, or expiring CA rather
  than silently replacing the phone's trust anchor.

## Upgrades, reinstalling, and recovery

Older releases used a directly self-signed leaf (`remote-cert-v4.der`) and
regenerated it whenever a newly observed IP was absent from a sidecar SAN list.
Even a virtual-adapter change could invalidate the certificate trusted by the
phone. Some releases also hid certificate setup and packaged the non-CA leaf
as a root profile. Upgrading from this format requires the one-time setup above.
Old files are left intact for rollback; they are not promoted to a CA.

Keep `remote-tls-identity-v1.json` with the application's user configuration
when backing up or reinstalling. On Windows it is in
`%APPDATA%\com.openless.app`; on Tauri macOS it is in the application's config
directory; on Linux egui it is in the host data directory's `remote-input`
subdirectory. This file contains private keys: do not publish it, send it to a
phone, or copy it to another computer. Unix files are owner-readable/writable
only; Windows files inherit the user's application-data ACL.

If this file is damaged, restore the computer's own backup. If it is lost, or
the CA expires (ten years), explicitly back up/remove the old identity while
OpenLess is stopped, restart, and install/trust the newly generated CA on each
phone. Deleting all application data necessarily loses the old trust identity.

## Regression checks

```sh
cargo test --manifest-path openless-all/app/src-tauri/backend-tests/Cargo.toml --test remote_tls
node openless-all/app/scripts/remote-input-audio-queue.test.mjs
```

The TLS tests exercise real rustls handshakes with only the downloaded CA as a
trust anchor, IP changes, restart reuse, renewal, corrupted keys, persistence
failure, migration, and separation between two computers. iOS installation and
microphone recording still require a physical-device smoke test.
