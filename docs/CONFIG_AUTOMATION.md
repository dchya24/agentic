# 🤖 Agentic Config Automation

Fitur otomatisasi setup config untuk Agentic AI di Termul dan Agentic CLI.

---

## 📋 Fitur Utama

### 1. **First-Run Setup Wizard** (Termul)
Setup wizard interaktif yang muncul saat user pertama kali membuka Termul.

**Fitur:**
- ✅ Multi-step wizard dengan progress
- ✅ Pilihan provider (OpenAI, Anthropic, Z.ai)
- ✅ Input API key dengan masking
- ✅ Pilihan model untuk setiap provider
- ✅ Konfigurasi safety settings
- ✅ Validasi config sebelum save
- ✅ Auto-create config directory
- ✅ First-run detection dengan localStorage

**Location:** `src/renderer/components/agentic/AgenticConfigWizard.tsx`

### 2. **First-Run Banner** (Termul)
Banner informatif yang ditampilkan jika config belum di-setup.

**Fitur:**
- ✅ Cek status config secara otomatis
- ✅ Quick setup untuk create default config
- ✅ Link ke Settings untuk edit manual
- ✅ Tombol dismiss untuk skip
- ✅ LocalStorage untuk track wizard seen

**Location:** `src/renderer/components/agentic/FirstRunBanner.tsx`

### 3. **CLI Config Commands** (Agentic CLI)
Commands lengkap untuk manage config dari CLI.

**Available Commands:**
```bash
# Initialize config file
agentic config init

# Show current config
agentic config show

# Edit config in default editor
agentic config edit

# Validate config
agentic config validate

# Reset config to defaults
agentic config reset [--force]

# Get config file path
agentic config path
```

**Location:** `agentic-cli/src/commands.rs`

---

## 🚀 Cara Kerja

### Termul App Flow

```
1. Buka Termul
   ↓
2. FirstRunBanner cek config status
   ↓
3. Jika config tidak ada:
   - Tampilkan banner dengan setup options
   - Auto-show setup wizard (jika first run)
   ↓
4. User pilih setup:
   a. Quick Setup → create default config
   b. Setup Wizard → guided multi-step setup
   ↓
5. Config disimpan ke ~/.config/agentic/config.json
   ↓
6. Agentic AI ready untuk digunakan
```

### CLI App Flow

```
1. Run: agentic run "list files"
   ↓
2. CLI cek config existence
   ↓
3. Jika config tidak ada:
   - Tampilkan error message
   - Suggest commands: init, edit, show
   ↓
4. User run: agentic config init
   ↓
5. Config dibuat dengan default values
   ↓
6. Prompt untuk setup API key (optional)
   ↓
7. Config ready, user bisa gunakan agentic commands
```

---

## 📁 Lokasi Config File

| Platform | Path |
|----------|------|
| Linux | `~/.config/agentic/config.json` |
| macOS | `~/.config/agentic/config.json` |
| Windows | `%USERPROFILE%\.config\agentic\config.json` |

---

## 🔧 Tauri Commands (Backend)

### Config Management

```rust
// Cek apakah config file exists
pub fn agentic_config_exists() -> bool

// Get config file path
pub fn agentic_config_path() -> String

// Create default config file
pub fn agentic_create_default_config() -> Result<Config, String>

// Load config from file
pub fn agentic_read_file_config() -> Result<Config, String>

// Save config to file
pub fn agentic_save_config(config: Config) -> Result<Config, String>

// Validate config
pub fn agentic_validate_config() -> Result<(bool, Vec<String>), String>

// Open config in default editor
pub fn agentic_open_config_editor() -> Result<(), String>

// Load config into agentic state
pub fn agentic_load_config(config: Config) -> Result<AgenticStatus, String>
```

### Location: `src-tauri/src/agentic/commands.rs`

---

## 💻 Frontend Helpers

### AgenticConfigHelper

```typescript
// Cek status config
export async function checkAgenticConfigStatus(): Promise<AgenticConfigStatus>

// Create default config
export async function createDefaultConfig(): Promise<void>

// Validate config
export async function validateConfig(): Promise<{ valid: boolean; errors: string[] }>

// Get config path
export async function getConfigPath(): Promise<string>

// Open config in editor
export async function openConfigInEditor(): Promise<void>

// Cek apakah harus show first-run wizard
export function shouldShowFirstRunWizard(): boolean

// Mark wizard sebagai seen
export function markWizardSeen(): void

// Reset wizard state
export function resetWizardState(): void
```

### Location: `src/renderer/lib/agentic-config-helper.ts`

---

## 📦 Core-Agentic Library Updates

### Config Functions

```rust
impl Config {
    // Cek apakah config file exists
    pub fn config_exists() -> bool;

    // Load config (returns None if not exists)
    pub fn load() -> Option<Self>;

    // Create default config file
    pub fn create_default() -> Result<Self, String>;

    // Get fallback/default config
    pub fn fallback() -> Self;

    // Validate config
    pub fn is_valid(&self) -> (bool, Vec<String>);

    // Save config
    pub fn save(&self) -> Result<(), String>;
}
```

### Location: `core-agentic/src/config.rs`

---

## 🎯 Use Cases

### Use Case 1: First-Time User (Termul)

```bash
# 1. Buka Termul
npm run dev

# 2. Setup wizard auto-appears
# - Select provider: OpenAI
# - Enter API key: sk-...
# - Select model: GPT-4o
# - Configure safety: default
# - Save

# 3. Config auto-created at ~/.config/agentic/config.json
# 4. Agentic AI ready to use!
```

### Use Case 2: First-Time User (CLI)

```bash
# 1. Try to use agentic
agentic run "list files"

# 2. Error: Config not found
# ℹ Config file not found at: ~/.config/agentic/config.json
#
# To get started:
#   1. Run 'agentic config init' to create a default config
#   2. Edit the config to add your API key
#   3. Or use environment variables: OPENAI_API_KEY and OPENAI_BASE_URL

# 3. Initialize config
agentic config init

# 4. Setup API key (prompted)
# ℹ Your API key is currently empty.
# Would you like to set it now? [Y/n]: y
# Enter your API key: sk-...
# ✓ API key saved!

# 5. Ready to use
agentic run "list files"
```

### Use Case 3: Advanced Configuration

```bash
# 1. Initialize config
agentic config init

# 2. Edit config in your editor
agentic config edit

# 3. Or view config
agentic config show

# 4. Validate config
agentic config validate
# ✓ Configuration is valid!

# 5. Reset if needed
agentic config reset --force
```

### Use Case 4: Environment Variables

```bash
# 1. Set environment variables
export OPENAI_API_KEY="sk-..."
export OPENAI_BASE_URL="https://api.openai.com/v1"

# 2. Initialize config (will use env vars)
agentic config init

# 3. Config will reference env vars
{
  "providers": [
    {
      "api_key": "$OPENAI_API_KEY",
      "api_base": "$OPENAI_BASE_URL",
      ...
    }
  ]
}
```

---

## 🔍 Config Structure

### Minimal Valid Config

```json
{
  "providers": [
    {
      "name": "openai",
      "type": "openai-compatible",
      "api_base": "https://api.openai.com/v1",
      "api_key": "sk-your-api-key",
      "models": [
        {
          "model": "gpt-4o",
          "temperature": 0.7,
          "max_tokens": 8192
        }
      ]
    }
  ],
  "safety": {
    "auto_approve_low_risk": true,
    "blocked_commands": ["rm -rf /", "mkfs", "dd if="]
  },
  "output": {
    "color": true,
    "stream": true,
    "show_thoughts": true,
    "show_tool_calls": true
  }
}
```

### Full Config with MCP Servers

```json
{
  "providers": [
    {
      "name": "openai",
      "type": "openai-compatible",
      "api_base": "https://api.openai.com/v1",
      "api_key": "sk-your-api-key",
      "models": [...]
    }
  ],
  "safety": {...},
  "output": {...},
  "mcp_servers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/home/user/projects"]
    },
    "my-api": {
      "url": "http://localhost:3001/mcp",
      "headers": {
        "Authorization": "Bearer my-token"
      }
    }
  }
}
```

---

## 🛠️ Implementation Details

### First-Run Detection

```typescript
// Uses localStorage to track:
// - first-visit-date: Date of first app launch
// - agentic-wizard-seen: Whether wizard was shown

// Wizard shown if:
// 1. Config doesn't exist AND
// 2. First visit within 7 days AND
// 3. Wizard not marked as seen
```

### Auto-Creation Flow

```typescript
// Termul:
// 1. App starts → FirstRunBanner checks config
// 2. No config → show banner
// 3. User clicks "Quick Setup" → invoke agentic_create_default_config()
// 4. Config created → show wizard for API key input

// CLI:
// 1. User runs command without config
// 2. Show helpful error message
// 3. User runs 'agentic config init'
// 4. Config created → prompt for API key
```

---

## ✅ Status

| Feature | Status | Notes |
|---------|--------|-------|
| First-Run Wizard | ✅ Implemented | Multi-step UI |
| First-Run Banner | ✅ Implemented | With quick setup |
| CLI config init | ✅ Implemented | With API key prompt |
| CLI config show | ✅ Implemented | Pretty JSON output |
| CLI config edit | ✅ Implemented | Opens default editor |
| CLI config validate | ✅ Implemented | Returns errors |
| CLI config reset | ✅ Implemented | With confirmation |
| CLI config path | ✅ Implemented | Returns path |
| Config auto-create | ✅ Implemented | Via create_default() |
| Config validation | ✅ Implemented | Via is_valid() |
| First-run detection | ✅ Implemented | LocalStorage based |
| Auto-open wizard | ✅ Implemented | On first visit |

---

## 📝 Todo / Future Enhancements

- [ ] Add config import/export
- [ ] Add config template selection
- [ ] Add config backup/restore
- [ ] Add multi-provider management UI
- [ ] Add MCP server management UI
- [ ] Add config migration handling
- [ ] Add config diff/compare
- [ ] Add cloud config sync (optional)
- [ ] Add config sharing/shipping templates
- [ ] Add config validation with API key test

---

## 🐛 Troubleshooting

### Config not loading

```bash
# Check config exists
agentic config path
ls -l ~/.config/agentic/config.json

# Validate config
agentic config validate

# If invalid, reset
agentic config reset
```

### Wizard not showing

```javascript
// Reset wizard state
localStorage.removeItem('agentic-wizard-seen')
localStorage.removeItem('first-visit-date')
// Reload app
```

### API key not working

```bash
# Check config
agentic config show

# Verify environment variables
echo $OPENAI_API_KEY
echo $OPENAI_BASE_URL

# Test API key
curl https://api.openai.com/v1/models \
  -H "Authorization: Bearer $OPENAI_API_KEY"
```

---

## 📚 References

- **Config Schema:** `docs/CONFIGURATION.md`
- **Core-Agentic Config:** `core-agentic/src/config.rs`
- **Agentic PRD:** `docs/AGENTIC_PRD.md`
- **CLI Commands:** `agentic-cli/src/commands.rs`
- **Tauri Commands:** `src-tauri/src/agentic/commands.rs`
