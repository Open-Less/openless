#include "rime_engine.h"
#include <rime_api.h>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <map>
#include <limits>
#include <mutex>
#include <stdexcept>
#include <string>

namespace {
std::mutex mutex;
RimeApi* api = nullptr;
std::string shared_dir, user_dir;
struct Session { std::string pending; unsigned long revision = 0; bool learning = false; };
std::map<RimeSessionId, Session> sessions;
std::string quote(const std::string& value) {
    std::string out = "\"";
    const char* hex = "0123456789abcdef";
    for (unsigned char c : value) {
        if (c == '"' || c == '\\') { out += '\\'; out += c; }
        else if (c < 32) { out += "\\u00"; out += hex[c >> 4]; out += hex[c & 15]; }
        else out += c;
    }
    return out + "\"";
}
std::string snapshot(RimeSessionId id, bool expanded = false, int offset = 0, int limit = 64) {
    auto& session = sessions.at(id);
    RIME_STRUCT(RimeCommit, commit);
    if (api->get_commit(id, &commit)) {
        if (commit.text) session.pending += commit.text;
        api->free_commit(&commit);
    }
    RIME_STRUCT(RimeContext, context);
    if (!api->get_context(id, &context)) throw std::runtime_error("Rime context unavailable");
    std::string out = "{\"ok\":true,\"session\":" + quote(std::to_string(id)) +
        ",\"learning\":" + (session.learning ? "true" : "false") +
        ",\"revision\":" + std::to_string(session.revision) +
        ",\"commit\":" + quote(session.pending) +
        ",\"input\":" + quote(api->get_input(id) ? api->get_input(id) : "") +
        ",\"preedit\":" + quote(context.composition.preedit ? context.composition.preedit : "") +
        ",\"page\":" + std::to_string(context.menu.page_no) +
        ",\"hasNext\":" + (context.menu.is_last_page ? "false" : "true") + ",\"candidates\":[";
    bool more = !context.menu.is_last_page;
    if (expanded) {
        // Android's existing overflow list needs more than a single menu page.
        // Keep the native order and use absolute indices for this bounded list.
        RimeCandidateListIterator iterator{};
        more = false;
        if (api->candidate_list_from_index(id, &iterator, offset)) {
            int count = 0;
            while (count < limit && api->candidate_list_next(&iterator)) {
                if (count++) out += ',';
                out += quote(iterator.candidate.text ? iterator.candidate.text : "");
            }
            more = count == limit && api->candidate_list_next(&iterator);
            api->candidate_list_end(&iterator);
        }
    } else {
        for (int i = 0; i < context.menu.num_candidates; ++i) {
            if (i) out += ',';
            out += quote(context.menu.candidates[i].text ? context.menu.candidates[i].text : "");
        }
    }
    out += "],\"candidateOffset\":" + std::to_string(expanded ? offset : context.menu.page_no * context.menu.page_size) +
        ",\"candidateHasNext\":" + (more ? "true" : "false") + "}";
    api->free_context(&context);
    return out;
}
std::string request(const std::string& op, const std::string& handle, const std::string& arg) {
    if (op == "initialize") {
        if (api) {
            if (shared_dir != handle || user_dir != arg) throw std::runtime_error("Rime already initialized with another profile");
            return "{\"ok\":true}";
        }
        if (handle.empty() || arg.empty()) throw std::runtime_error("Rime data directories required");
        shared_dir = handle; user_dir = arg;
        std::filesystem::create_directories(user_dir);
        auto candidate = rime_get_api();
        RIME_STRUCT(RimeTraits, traits);
        traits.shared_data_dir = shared_dir.c_str(); traits.user_data_dir = user_dir.c_str();
        traits.distribution_name = "OpenLess"; traits.distribution_code_name = "openless";
        traits.distribution_version = "1.17.0"; traits.app_name = "rime.openless";
        candidate->setup(&traits);
        candidate->initialize(&traits);
        if (candidate->start_maintenance(True)) candidate->join_maintenance_thread();
        for (const auto* name : {"openless_pinyin_private", "openless_pinyin_learning"}) {
            const auto schema = shared_dir + "/" + name + ".schema.yaml";
            if (!candidate->deploy_schema(schema.c_str())) {
                candidate->finalize();
                throw std::runtime_error("Rime schema deployment failed");
            }
        }
        api = candidate;
        return "{\"ok\":true}";
    }
    if (!api) throw std::runtime_error("Rime is not initialized");
    if (op == "create") {
        auto id = api->create_session();
        if (!id) throw std::runtime_error("Rime session creation failed");
        if (!api->select_schema(id, arg == "learning" ? "openless_pinyin_learning" : "openless_pinyin_private")) {
            api->destroy_session(id);
            throw std::runtime_error("Rime pinyin_simp schema unavailable");
        }
        api->set_option(id, "ascii_mode", False);
        // Sessions with learning disabled must not update the user dictionary.
        Session session;
        session.learning = arg == "learning";
        sessions.emplace(id, session);
        return snapshot(id);
    }
    size_t consumed = 0;
    auto id = static_cast<RimeSessionId>(std::stoull(handle, &consumed));
    if (consumed != handle.size() || !sessions.count(id) || !api->find_session(id))
        throw std::runtime_error("Rime session is stale");
    if (op == "destroy") {
        api->destroy_session(id); sessions.erase(id); return "{\"ok\":true}";
    }
    auto& session = sessions.at(id);
    int candidate_offset = 0;
    int candidate_limit = 64;
    if (op == "candidates" && !arg.empty()) {
        const auto separator = arg.find('@');
        if (separator == std::string::npos) throw std::runtime_error("Rime candidate revision required");
        size_t n = 0, r = 0;
        const auto offset = std::stoul(arg.substr(0, separator), &n);
        const auto revision = std::stoul(arg.substr(separator + 1), &r);
        if (n != separator || r != arg.size() - separator - 1 || revision != session.revision ||
            offset > static_cast<unsigned long>(std::numeric_limits<int>::max()))
            throw std::runtime_error("Rime candidate range is stale or invalid");
        candidate_offset = static_cast<int>(offset);
        candidate_limit = 32;
    }
    if (op == "ack") session.pending.clear();
    else if (op == "clear") { api->clear_composition(id); session.pending.clear(); }
    else if (op != "state" && op != "candidates") {
        if (!session.pending.empty()) throw std::runtime_error("Rime commit requires acknowledgement");
        if (op == "key") {
            size_t n = 0; auto key = std::stoi(arg, &n);
            if (n != arg.size()) throw std::runtime_error("Invalid Rime key");
            api->process_key(id, key, 0);
        } else if (op == "external") {
            const auto separator = arg.find('@');
            if (separator == std::string::npos) throw std::runtime_error("External candidate revision required");
            size_t n = 0;
            const auto revision = std::stoul(arg.substr(0, separator), &n);
            const auto text = arg.substr(separator + 1);
            if (n != separator || revision != session.revision || text.empty() || text.size() > 512 ||
                text.find_first_of("\r\n\t") != std::string::npos)
                throw std::runtime_error("External candidate is stale or invalid");
            api->clear_composition(id);
            session.pending = text;
        } else if (op == "confirm") {
            api->commit_composition(id);
        } else if (op == "select" || op == "select_absolute") {
            const auto separator = arg.find('@');
            if (separator == std::string::npos) throw std::runtime_error("Rime candidate revision required");
            size_t n = 0; auto index = std::stoul(arg.substr(0, separator), &n);
            size_t r = 0; auto revision = std::stoul(arg.substr(separator + 1), &r);
            if (n != separator || r != arg.size() - separator - 1 || revision != session.revision ||
                !(op == "select_absolute" ? api->select_candidate(id, index) : api->select_candidate_on_current_page(id, index)))
                throw std::runtime_error("Rime candidate is stale");
        } else throw std::runtime_error("Unknown Rime operation");
    }
    // Observing a composition must not invalidate the displayed candidates.
    if (op != "state" && op != "candidates") ++session.revision;
    return snapshot(id, op == "candidates", candidate_offset, candidate_limit);
}
}
extern "C" char* openless_rime_request(const char* operation, const char* session, const char* argument) {
    std::lock_guard<std::mutex> lock(mutex);
    std::string response;
    try { response = request(operation ? operation : "", session ? session : "", argument ? argument : ""); }
    catch (const std::exception& error) { response = "{\"ok\":false,\"error\":" + quote(error.what()) + "}"; }
    auto result = static_cast<char*>(std::malloc(response.size() + 1));
    if (result) std::memcpy(result, response.c_str(), response.size() + 1);
    return result;
}
extern "C" void openless_rime_free(char* response) { std::free(response); }
