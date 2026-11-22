# OpenLess AOCI Code 开发文档

AOCI Code 为助手提供可增量维护的代码索引。索引描述文件的职责、重要依赖、调用入口和容易忽略的实现约束。源码和运行结果提供实际行为的证据。

## 文件与工作区

| 路径 | 用途 |
| --- | --- |
| `aoci.txt` | 索引总清单，声明项目、索引格式和各卷的位置 |
| `aoci.meta.txt` | Meta 卷，存放仓库级约定和索引词典 |
| `aoci.code.txt` | Code 卷，按组记录纳入索引的文件 |
| `.aoci/config.json` | 项目配置，定义管理范围、维护模式等 |
| `.aoci/baseline.json` | 文件指纹基线，用来判断源码是否发生变化 |

每个 Git 工作区都以自己的源码和基线为准。已有索引可作为同项目检出的起点，复制时保留清单、卷、配置和基线之间的一致性，随后对当前源码重新核验。维护日志和事务记录由工具留在本地。

Codex 可在工作区的 `.codex/config.toml` 配置 MCP，Claude Code 可在 `.mcp.json` 配置同一服务。MCP 是助手调用工具的接口。服务命令为 `aoci --repo <当前工作区根目录> mcp`，配置里的二进制应支持当前的索引格式。切换工作区时，用 `aoci doctor` 核对识别到的目录和集成，用 `codex mcp list` 核对 Codex 是否加载了项目服务。

## 工具与字段

| 工具 | 用法 |
| --- | --- |
| `aoci_rules` | 读取当前服务的会话规则。以实际服务器版本的定义和返回为准 |
| `aoci_overview` | 加载或恢复仓库全局索引。需要续页时沿 `next_cursor` 读到完成，再按返回的校验要求确认 |
| `aoci_search`、`aoci_get_entries` | 查找相关索引条目，随后阅读源码确认细节 |
| `aoci_maintain` | 列出当前工作区的待维护条目、移除候选及需要复核的观察项 |
| `aoci_update_entry` | 提交完整批次，保留服务返回的候选标识、源码哈希与批次标识 |
| `aoci verify` | 核验索引结构与源码对齐状态，可能写入本地验收记录 |

普通 Code 维护用空参数调用 `aoci_maintain`。当它返回待更新条目时，`aoci_update_entry` 的 `code_batch_id` 取自 `code_plan.batch_id`。在 `entries` 中逐条复制 `path`、`candidate_id` 和 `source_sha256`，并提供整行 `new_entry`。如果同批有 `orphan_remove_candidates`，按返回路径填入 `removals`。对齐后若要修正已有条目的描述或引用，可用 `aoci_maintain` 的 `cognition_optimization` 模式指定 `object_refs`。

Code 卷的一条记录包含文件名、标签和四个字段。标签使用 Meta 卷定义的词典。

| 字段 | 含义 |
| --- | --- |
| `F` | 文件负责什么 |
| `R` | 理解或修改该文件时需要关注的关系，`code:<仓库相对路径>` 指向具体文件 |
| `A` | 对读者有用的入口、接口、配置项或命名实体 |
| `S` | 源码中有依据的约束、设计原因或兼容性要求 |

## 更新流程

1. 确认仓库、工作区和服务版本。首次建立索引、迁移、变更管理范围及恢复事务时，使用当前服务返回的 Guide。
2. 待文件稳定后调用 `aoci_maintain`。阅读候选所绑定的源码和必要的相关文件，再撰写条目。文件名、符号列表和依赖扫描可以帮助定位证据。
3. 用 `aoci_update_entry` 提交整个批次。收到 `repair_required` 时只修正指出的条目，再提交同一个完整批次。应用成功且 `remaining` 大于零时，再取下一批。观察项、冲突、恢复和审批按服务返回的证据与命令处理。
4. 检查最终提交结果的 `terminal_proof` 是否确认结构有效和源码对齐。也可独立运行 `aoci verify --json`。之后又改动受管理文件时，对新状态重新核验。

全局索引依赖当前工作区和索引版本的读取凭据。发生上下文压缩时，按 `aoci_rules` 的要求重新读取并确认完整索引。只读查询或无受管文件改动的任务可以直接收尾。
