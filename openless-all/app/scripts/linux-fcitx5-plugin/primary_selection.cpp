/*
 * SPDX-FileCopyrightText: 2025 OpenLess Contributors
 *
 * SPDX-License-Identifier: LGPL-2.1-or-later
 *
 * `primary_selection.h` 的实现：用 ext-data-control 读一次合成器上的 PRIMARY。
 *
 * 每次调用建立一条独立的 Wayland 连接（连接 + 3 次 roundtrip 约 1ms 量级），
 * 用完即断：不需要事件循环、不占用线程，也不会有"连接建立时的选区状态"
 * 与"现在的选区状态"不一致的问题。代价是每次捕获都新建连接，这远小于一次
 * 热键动作的时延预算。
 */
#include "primary_selection.h"

#include <fcntl.h>
#include <poll.h>
#include <unistd.h>

#include <algorithm>
#include <cerrno>
#include <chrono>
#include <cstdlib>
#include <cstring>
#include <string>
#include <utility>
#include <vector>

#include <wayland-client.h>

#include "ext-data-control-v1-client-protocol.h"

namespace openless_selection {

namespace {

/// KDE/密码管理器约定：带这个 mime 的选区说明来源应用认为内容是密码。
/// 值为 "secret" 时不读（与 fcitx5 clipboard 模块的默认行为一致）。
constexpr char kPasswordMimeType[] = "x-kde-passwordManagerHint";
constexpr char kUtf8MimeType[] = "text/plain;charset=utf-8";
constexpr char kTextMimeType[] = "text/plain";
/// 读密码提示本身是个额外的小传输，给它一个更短的预算，别把总时延拉长。
constexpr int kPasswordHintReadTimeoutMs = 50;

/// 一次探针用到的全部 Wayland 对象；析构即清理干净。
struct ProbeConnection {
    wl_display *display = nullptr;
    wl_registry *registry = nullptr;
    ext_data_control_manager_v1 *manager = nullptr;
    wl_seat *seat = nullptr;
    ext_data_control_device_v1 *device = nullptr;
    /// 合成器发过 finished：data device 已失效，这次拿不到任何结论。
    bool deviceFinished = false;
    /// 是否收到过 primary_selection 事件。协议规定绑定 device 时若合成器支持
    /// primary selection 就会立刻补发一次，所以没收到 = 这个合成器不支持。
    bool primaryEventSeen = false;
    bool hasPrimaryOffer = false;
    ext_data_control_offer_v1 *primaryOffer = nullptr;
    const std::vector<std::string> *primaryMimes = nullptr;
    /// data_offer 事件发来的 offer 及其实例，按到达顺序记录（primary offer 靠它查 mime）。
    std::vector<std::pair<ext_data_control_offer_v1 *, std::vector<std::string> *>>
        offers;

    ~ProbeConnection() {
        for (auto &[offer, mimes] : offers) {
            ext_data_control_offer_v1_destroy(offer);
            delete mimes;
        }
        if (device != nullptr) {
            ext_data_control_device_v1_destroy(device);
        }
        if (manager != nullptr) {
            ext_data_control_manager_v1_destroy(manager);
        }
        if (seat != nullptr) {
            wl_seat_destroy(seat);
        }
        if (registry != nullptr) {
            wl_registry_destroy(registry);
        }
        if (display != nullptr) {
            wl_display_disconnect(display);
        }
    }
};

void handleRegistryGlobal(void *data, wl_registry *registry, uint32_t name,
                          const char *interface, uint32_t version) {
    auto *probe = static_cast<ProbeConnection *>(data);
    if (std::strcmp(interface, ext_data_control_manager_v1_interface.name) == 0) {
        probe->manager = static_cast<ext_data_control_manager_v1 *>(
            wl_registry_bind(registry, name,
                             &ext_data_control_manager_v1_interface,
                             std::min<uint32_t>(version, 1)));
    } else if (std::strcmp(interface, wl_seat_interface.name) == 0) {
        if (probe->seat == nullptr) {
            probe->seat = static_cast<wl_seat *>(
                wl_registry_bind(registry, name, &wl_seat_interface,
                                 std::min<uint32_t>(version, 7)));
        }
    }
}

void handleRegistryGlobalRemove(void *, wl_registry *, uint32_t) {}

const wl_registry_listener kRegistryListener = {
    .global = handleRegistryGlobal,
    .global_remove = handleRegistryGlobalRemove,
};

void handleOfferMime(void *data, ext_data_control_offer_v1 *, const char *mimeType) {
    static_cast<std::vector<std::string> *>(data)->emplace_back(mimeType);
}

const ext_data_control_offer_v1_listener kOfferListener = {
    .offer = handleOfferMime,
};

void handleDeviceDataOffer(void *data, ext_data_control_device_v1 *,
                           ext_data_control_offer_v1 *offer) {
    auto *probe = static_cast<ProbeConnection *>(data);
    auto *mimes = new std::vector<std::string>();
    ext_data_control_offer_v1_add_listener(offer, &kOfferListener, mimes);
    probe->offers.emplace_back(offer, mimes);
}

/// 剪贴板（CLIPBOARD）选区：本插件只关心 PRIMARY，忽略但要收下事件。
void handleDeviceSelection(void *, ext_data_control_device_v1 *,
                           ext_data_control_offer_v1 *) {}

void handleDeviceFinished(void *data, ext_data_control_device_v1 *device) {
    auto *probe = static_cast<ProbeConnection *>(data);
    probe->deviceFinished = true;
    // 协议要求客户端销毁它；这里同步销毁，避免析构时二次 destroy。
    ext_data_control_device_v1_destroy(device);
    probe->device = nullptr;
}

void handleDevicePrimarySelection(void *data, ext_data_control_device_v1 *,
                                  ext_data_control_offer_v1 *offer) {
    auto *probe = static_cast<ProbeConnection *>(data);
    probe->primaryEventSeen = true;
    probe->hasPrimaryOffer = offer != nullptr;
    probe->primaryOffer = offer;
    probe->primaryMimes = nullptr;
    if (offer == nullptr) {
        return;
    }
    for (const auto &[recorded, mimes] : probe->offers) {
        if (recorded == offer) {
            probe->primaryMimes = mimes;
            break;
        }
    }
}

const ext_data_control_device_v1_listener kDeviceListener = {
    .data_offer = handleDeviceDataOffer,
    .selection = handleDeviceSelection,
    .finished = handleDeviceFinished,
    .primary_selection = handleDevicePrimarySelection,
};

bool containsMime(const std::vector<std::string> *mimes, const char *mime) {
    return mimes != nullptr &&
           std::find(mimes->begin(), mimes->end(), mime) != mimes->end();
}

std::string joinMimes(const std::vector<std::string> *mimes) {
    std::string joined;
    if (mimes == nullptr) {
        return joined;
    }
    for (const auto &mime : *mimes) {
        if (!joined.empty()) {
            joined += ", ";
        }
        joined += mime;
    }
    return joined;
}

/// 从管道读到 EOF；`timeoutMs` 是等待第一份数据和后续每次可读的上限。
bool readPipe(int readFd, int timeoutMs, std::string &out) {
    std::string data;
    char chunk[4096];
    const auto deadline =
        std::chrono::steady_clock::now() + std::chrono::milliseconds(timeoutMs);
    for (;;) {
        const auto remaining = std::chrono::duration_cast<std::chrono::milliseconds>(
                                   deadline - std::chrono::steady_clock::now())
                                   .count();
        if (remaining <= 0) {
            return false;
        }
        pollfd pollFd{readFd, POLLIN, 0};
        const int ready = ::poll(&pollFd, 1, static_cast<int>(remaining));
        if (ready <= 0) {
            return false;
        }
        const ssize_t count = ::read(readFd, chunk, sizeof(chunk));
        if (count > 0) {
            data.append(chunk, static_cast<size_t>(count));
            continue;
        }
        if (count == 0) {
            break;
        }
        if (errno == EINTR) {
            continue;
        }
        return false;
    }
    out = std::move(data);
    return true;
}

/// 让来源应用把某个 mime 的数据写进管道并读出来。失败（超时/IO）返回 false。
bool readOfferData(ext_data_control_offer_v1 *offer, wl_display *display,
                   const char *mime, int timeoutMs, std::string &out) {
    int fds[2];
    if (::pipe2(fds, O_CLOEXEC) != 0) {
        return false;
    }
    ext_data_control_offer_v1_receive(offer, mime, fds[1]);
    ::close(fds[1]);
    if (wl_display_flush(display) < 0) {
        ::close(fds[0]);
        return false;
    }
    const bool ok = readPipe(fds[0], timeoutMs, out);
    ::close(fds[0]);
    return ok;
}

} // namespace

SelectionSource chooseSelectionSource(
    const PrimarySelectionSnapshot &primary,
    const std::string &surroundingSelectedText) {
    // 1. 探针读到了文本：这是此刻的合成器真值，直接用。
    //    surrounding 即便非空也不参与取值（③）：它是客户端自己报的本地缓存
    //    （fcitx5 的头文件原话是 "Local cache for surrounding text"），
    //    可能没跟上用户刚刚的选区变化，甚至可能只覆盖 4000 字节以内的一段。
    if (primary.status == PrimarySelectionStatus::Text && !primary.text.empty()) {
        return {primary.text, "primary"};
    }
    // 2. 探针明确说"有选区但没有 text mime"（图片/文件/密码）→ 判失效。
    //    这里**故意**不回退 fcitx5 缓存，也不回退 surrounding：前者是上一次的
    //    文本（② 要修的就是它），后者会让"选中图片"这类情况重新冒出一段文本选区。
    if (primary.status == PrimarySelectionStatus::NoText) {
        return {std::string(), "no-text-selection"};
    }
    // 3. 探针没能给出结论（没有 Wayland / 合成器不提供 ext-data-control /
    //    合成器不支持 primary selection / 没读完）：用应用自己报的选中文本。
    //    很多应用从不导出 PRIMARY（XIM、部分工具包），这条路径就是给它们的。
    if (!surroundingSelectedText.empty()) {
        return {surroundingSelectedText,
                primary.status == PrimarySelectionStatus::Unsupported
                    ? "surrounding-fallback-unavailable"
                    : "surrounding-fallback"};
    }
    return {std::string(), "none"};
}

PrimarySelectionSnapshot readPrimarySelection() {
    PrimarySelectionSnapshot snapshot;
    const char *displayName = std::getenv("WAYLAND_DISPLAY");
    if (displayName == nullptr || *displayName == '\0') {
        snapshot.detail = "WAYLAND_DISPLAY is not set";
        return snapshot;
    }

    ProbeConnection probe;
    probe.display = wl_display_connect(nullptr);
    if (probe.display == nullptr) {
        snapshot.detail = "wl_display_connect failed";
        return snapshot;
    }
    probe.registry = wl_display_get_registry(probe.display);
    wl_registry_add_listener(probe.registry, &kRegistryListener, &probe);
    if (wl_display_roundtrip(probe.display) < 0) {
        snapshot.detail = "registry roundtrip failed";
        return snapshot;
    }
    if (probe.manager == nullptr) {
        snapshot.detail = "compositor has no ext_data_control_manager_v1";
        return snapshot;
    }
    if (probe.seat == nullptr) {
        snapshot.detail = "compositor has no wl_seat";
        return snapshot;
    }
    probe.device =
        ext_data_control_manager_v1_get_data_device(probe.manager, probe.seat);
    ext_data_control_device_v1_add_listener(probe.device, &kDeviceListener, &probe);
    if (wl_display_roundtrip(probe.display) < 0) {
        snapshot.detail = "data device roundtrip failed";
        return snapshot;
    }
    if (probe.deviceFinished) {
        snapshot.detail = "data device finished";
        return snapshot;
    }
    if (!probe.primaryEventSeen) {
        snapshot.detail = "compositor does not support primary selection";
        return snapshot;
    }
    if (!probe.hasPrimaryOffer || probe.primaryOffer == nullptr) {
        snapshot.status = PrimarySelectionStatus::NoSelection;
        snapshot.detail = "no primary selection";
        return snapshot;
    }

    snapshot.detail = joinMimes(probe.primaryMimes);
    const bool hasUtf8Mime = containsMime(probe.primaryMimes, kUtf8MimeType);
    const bool hasTextMime = containsMime(probe.primaryMimes, kTextMimeType);
    if (containsMime(probe.primaryMimes, kPasswordMimeType)) {
        std::string hint;
        if (readOfferData(probe.primaryOffer, probe.display, kPasswordMimeType,
                          kPasswordHintReadTimeoutMs, hint)) {
            while (!hint.empty() && hint.back() == '\0') {
                hint.pop_back();
            }
            if (hint == "secret") {
                snapshot.status = PrimarySelectionStatus::NoText;
                snapshot.detail = "password manager marked the selection as secret";
                return snapshot;
            }
        }
    }
    if (!hasUtf8Mime && !hasTextMime) {
        snapshot.status = PrimarySelectionStatus::NoText;
        return snapshot;
    }

    std::string data;
    if (!readOfferData(probe.primaryOffer, probe.display,
                       hasUtf8Mime ? kUtf8MimeType : kTextMimeType,
                       kPrimarySelectionReadTimeoutMs, data)) {
        snapshot.status = PrimarySelectionStatus::ReadFailed;
        snapshot.detail += " (read timed out)";
        return snapshot;
    }
    while (!data.empty() && data.back() == '\0') {
        data.pop_back();
    }
    snapshot.status = PrimarySelectionStatus::Text;
    snapshot.text = std::move(data);
    return snapshot;
}

} // namespace openless_selection
