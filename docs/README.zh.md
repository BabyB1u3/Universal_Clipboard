# Universal Clipboard

局域网跨设备剪贴板同步工具，用 Rust 编写。在同一局域网下的多台设备之间自动同步剪贴板内容，无需云服务，数据不出本地网络。

> **当前状态**：MVP 开发中，核心同步流程已可用，认证与加密功能正在实现。

[English](../README.md)

---

## 功能特性

- **自动发现**：通过 mDNS 自动发现局域网内的其他设备，无需手动配置 IP
- **显式配对**：基于公钥的设备配对机制，只同步来自已信任设备的内容
- **去重防环**：三层去重策略，防止剪贴板内容在设备间无限循环广播
- **跨平台**：支持 Linux、macOS、Windows（依赖 `arboard` 剪贴板库）
- **轻量守护进程**：无 GUI，以守护进程方式运行

---

## 架构

项目为 Cargo Workspace，包含 7 个功能 crate：

```
uniclip-daemon        ← 主程序入口（可执行文件）
├── uniclip-core      ← 核心数据结构、Blake3 内容哈希、LRU 去重缓存
├── uniclip-crypto    ← Ed25519 设备身份密钥管理
├── uniclip-proto     ← bincode 二进制线上协议
├── uniclip-store     ← JSON 配置文件与信任关系持久化
├── uniclip-discovery ← mDNS 服务广播与发现
└── uniclip-transport ← TCP 对等连接生命周期管理
```

### 同步流程

```
设备 A（用户复制文本）
  ↓
[Watcher] 每 250ms 轮询剪贴板，检测内容变化
  ↓
生成 ClipboardItem：event_id（UUID v4）+ content_hash（Blake3）
  ↓
[PeerManager] 通过 TCP 广播给所有已连接的受信设备
  ↓
设备 B [Listener] 接收 → 验证身份 → 去重检查 → 写入本地剪贴板
  ↓
设备 B [Watcher] 检测到内容变化，但命中回环抑制缓存 → 不再广播
```

---

## 快速开始

### 构建

```bash
git clone <repo>
cd Universal_Clipboard
cargo build --release
```

可执行文件位于 `target/release/uniclip-daemon`。

### 配对两台设备

在**设备 A** 上获取配对信息：

```bash
uniclip-daemon show-pairing 7878
# 输出类似：
# {"device_id":"xxxxxxxx-...","device_name":"my-laptop","pubkey_b64":"..."}
```

在**设备 B** 上导入设备 A：

```bash
uniclip-daemon pair 7879 '{"device_id":"...","device_name":"...","pubkey_b64":"..."}'
```

在**设备 A** 上导入设备 B（双向配对）：

```bash
uniclip-daemon pair 7878 '{"device_id":"...","device_name":"...","pubkey_b64":"..."}'
```

### 启动守护进程

```bash
# 设备 A
uniclip-daemon run 7878

# 设备 B
uniclip-daemon run 7879
```

启动后两台设备会通过 mDNS 自动发现彼此并建立连接，之后复制任意文本即可自动同步。

### 手动指定对端（跳过 mDNS）

```bash
uniclip-daemon run 7878 192.168.1.100:7879
```

---

## 配置文件

配置存储在系统标准配置目录下：

| 平台 | 路径 |
|------|------|
| Linux | `~/.config/uniclip/` |
| macOS | `~/Library/Application Support/uniclip/` |
| Windows | `%APPDATA%\uniclip\uniclip\` |

| 文件 | 内容 |
|------|------|
| `config.json` | 设备 ID、监听端口、已信任设备列表 |
| `identity.key` | Ed25519 私钥（base64，请妥善保管） |

---

## 安全说明

当前版本为 **MVP 阶段**，存在以下已知安全限制：

| 限制 | 状态 |
|------|------|
| TCP 传输未加密 | 计划引入 Noise Protocol |
| 消息来源仅凭 device_id 判断，未做签名验证 | Ed25519 基础设施已就绪，正在接入 |
| 私钥以明文 base64 存储 | 计划接入系统密钥链 |

**不建议在不可信的公共网络上使用当前版本。**

---

## 协议说明

### 服务发现

- 服务类型：`_uniclip._tcp.local.`
- TXT 记录：`device_id`、`device_name`

### 线上消息格式

所有消息以 `bincode` 序列化，外层为 4 字节大端长度前缀的帧格式（最大帧 2MB）：

```
[4 bytes: payload length] [N bytes: bincode(WireMessage)]
```

当前支持的消息类型：
- `Hello { version, device }` — 连接建立握手
- `ClipboardPush { item }` — 剪贴板内容推送

---

## 开发

```bash
# 运行所有检查
cargo check --workspace

# 构建
cargo build --workspace

# 格式化
cargo fmt --all

# Lint
cargo clippy --workspace
```

### Crate 依赖关系

```
uniclip-daemon
  └─ uniclip-{core, crypto, proto, store, discovery, transport}

uniclip-transport
  └─ uniclip-{core, proto}

uniclip-store
  └─ uniclip-{core, crypto}

uniclip-proto
  └─ uniclip-core
```

---

## 路线图

- [ ] Ed25519 消息签名验证
- [ ] Noise Protocol 传输加密
- [ ] 配置文件原子写入
- [ ] 系统密钥链存储私钥
- [ ] 图片/二进制剪贴板内容支持
- [ ] REST API（`uniclip-api` crate）
- [ ] 单元测试与集成测试

---

## License

待定。
