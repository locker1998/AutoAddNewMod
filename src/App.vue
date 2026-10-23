<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import LogicEditor from "./components/LogicEditor.vue";

type InboxItemKind = "archive" | "encrypted" | "folder" | "unknown";
type Tab = "drop" | "inbox" | "settings";

interface InboxItem {
  name: string;
  path: string;
  kind: InboxItemKind;
  size: number;
  gameId?: string | null;
  author?: string | null;
  requiresDecryption: boolean;
  hasDecryptKey: boolean;
  hasDecryptor: boolean;
  ready: boolean;
}

interface LogicFieldSpec {
  id: string;
  kind: string;
  label: string;
  accept: string[];
  secret: boolean;
  required: boolean;
  hidden?: boolean;
}

interface ModLogicView {
  extract: string;
  place: string;
  matchExt: string[];
  matchMime: string[];
  fields: LogicFieldSpec[];
}

interface GameAuthorSettings {
  id: string;
  name: string;
  authMode?: string;
  keyPath?: string | null;
  passwords?: string[];
  fieldValues: Record<string, string>;
  customLogic: string;
}

interface GameConfig {
  id: string;
  name: string;
  modFolder: string;
  organizeByAuthor: boolean;
  authors: GameAuthorSettings[];
}

interface AppConfig {
  games: GameConfig[];
  incomingDirectory?: string | null;
  authorUserNames?: Record<string, string>;
  authorKeys?: Record<string, string>;
  authorPasswords?: Record<string, string[]>;
  lastGameId?: string | null;
  lastAuthor?: string | null;
}

interface ExtractItemResult {
  name: string;
  success: boolean;
  installedMods: string[];
  error?: any;
  needsPassword: boolean;
  message?: string | null;
}

interface ProcessInboxResult {
  results: ExtractItemResult[];
  installedMods: string[];
}

const tab = ref<Tab>("drop");
const items = ref<InboxItem[]>([]);
const config = ref<AppConfig | null>(null);
const logicTemplate = ref("");
const authorLogicMap = ref<Record<string, ModLogicView>>({});
const selected = ref<string[]>([]);
const dragging = ref(false);
const status = ref("");
const busy = ref(false);

const passwordModal = ref(false);
const passwordFor = ref<string[]>([]);
const passwordValue = ref("");

const addGameModal = ref(false);
const addGameName = ref("");
const removeGameModal = ref(false);
const removeGameTarget = ref<{ id: string; name: string } | null>(null);
const expandedGames = ref<Set<string>>(new Set());

const addAuthorModal = ref(false);
const addAuthorGame = ref<GameConfig | null>(null);
const addAuthorName = ref("");

const logicModal = ref(false);
const logicAuthor = ref<GameAuthorSettings | null>(null);
const logicDraft = ref("");
const logicError = ref("");
const processAlert = ref("");

let unlistenInbox: UnlistenFn | null = null;
let unlistenDrop: UnlistenFn | null = null;

function toggleGameExpanded(id: string) {
  const next = new Set(expandedGames.value);
  if (next.has(id)) next.delete(id);
  else next.add(id);
  expandedGames.value = next;
}

function isGameExpanded(id: string) {
  return expandedGames.value.has(id);
}

function slugifyAuthorName(name: string) {
  return name
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "");
}

function ensureAuthorShape(author: GameAuthorSettings) {
  if (!author.fieldValues) author.fieldValues = {};
  if (!author.customLogic) author.customLogic = "";
}

function fieldsForAuthor(author: GameAuthorSettings): LogicFieldSpec[] {
  return (authorLogicMap.value[author.id]?.fields || []).filter((f) => !f.hidden);
}

async function refreshAuthorLogic(author: GameAuthorSettings) {
  ensureAuthorShape(author);
  try {
    authorLogicMap.value = {
      ...authorLogicMap.value,
      [author.id]: await invoke<ModLogicView>("parse_author_logic", {
        source: author.customLogic || "",
      }),
    };
  } catch {
    authorLogicMap.value = {
      ...authorLogicMap.value,
      [author.id]: {
        extract: "auto",
        place: "author",
        matchExt: [],
        matchMime: [],
        fields: [],
      },
    };
  }
}

async function refreshAllAuthorLogic() {
  if (!config.value) return;
  for (const g of config.value.games) {
    for (const a of g.authors) await refreshAuthorLogic(a);
  }
}

const authorOptions = computed(() => {
  const seen = new Set<string>();
  const out: { value: string; label: string }[] = [];
  for (const g of config.value?.games || []) {
    for (const a of g.authors) {
      const key = a.name.toLowerCase();
      if (seen.has(key)) continue;
      seen.add(key);
      out.push({ value: a.name, label: a.name });
    }
  }
  return out;
});

function authorOptionsForGameId(gameId?: string | null) {
  if (!gameId || !config.value) return [];
  const game = config.value.games.find((g) => g.id === gameId);
  if (!game) return [];
  return game.authors.map((a) => ({ value: a.name, label: a.name }));
}

const allSelected = computed(
  () => items.value.length > 0 && selected.value.length === items.value.length
);

const someSelected = computed(
  () => selected.value.length > 0 && selected.value.length < items.value.length
);

function isSelected(name: string) {
  return selected.value.includes(name);
}

async function refreshAll() {
  config.value = await invoke<AppConfig>("get_config");
  logicTemplate.value = await invoke<string>("mod_logic_template");
  for (const g of config.value.games) {
    if (!Array.isArray(g.authors)) g.authors = [];
    for (const a of g.authors) ensureAuthorShape(a);
  }
  await refreshAllAuthorLogic();
  items.value = await invoke<InboxItem[]>("list_inbox");
}

async function refreshInbox() {
  items.value = await invoke<InboxItem[]>("list_inbox");
}

function toggleSelect(name: string) {
  if (selected.value.includes(name)) {
    selected.value = selected.value.filter((n) => n !== name);
  } else {
    selected.value = [...selected.value, name];
  }
}

function toggleSelectAll() {
  if (allSelected.value) selected.value = [];
  else selected.value = items.value.map((i) => i.name);
}

function clearSelection() {
  selected.value = [];
}

function gameName(id?: string | null) {
  if (!id || !config.value) return "";
  return config.value.games.find((g) => g.id === id)?.name || id;
}

function statusLabel(item: InboxItem) {
  if (item.ready) return "Ready";
  if (!item.gameId || !item.author) return "Set game + author";
  if (!item.hasDecryptKey) return "Needs setup";
  return "Waiting";
}

function rowPreview(item: InboxItem) {
  const parts = [
    kindLabel(item.kind),
    formatSize(item.size),
    item.author || null,
    statusLabel(item),
  ].filter(Boolean);
  return parts.join(" · ");
}

async function importPaths(paths: string[]) {
  if (!paths.length) return;
  busy.value = true;
  try {
    await invoke("import_to_inbox", { paths });
    status.value = `Imported ${paths.length} item(s)`;
    await refreshInbox();
    tab.value = "inbox";
  } catch (e) {
    status.value = `Import failed: ${e}`;
  } finally {
    busy.value = false;
  }
}

async function onPickFiles() {
  const picked = await open({
    multiple: true,
    directory: false,
    title: "Select mod files",
  });
  const paths = Array.isArray(picked) ? picked : picked ? [picked] : [];
  await importPaths(paths);
}

async function onPickFolders() {
  const dirs = await open({
    multiple: true,
    directory: true,
    title: "Select mod folders",
  });
  const paths = Array.isArray(dirs) ? dirs : dirs ? [dirs] : [];
  await importPaths(paths);
}

async function setMeta(names: string[], gameId?: string | null, author?: string | null) {
  if (!names.length) return;
  await invoke("set_inbox_meta", {
    names,
    gameId: gameId ?? null,
    author: author ?? null,
  });
  await refreshInbox();
}

async function onGameChange(name: string, gameId: string) {
  // Changing game clears author so the dropdown only shows authors for that game.
  await setMeta([name], gameId || null, "");
}

async function onAuthorChange(name: string, author: string) {
  await setMeta([name], undefined, author || null);
}

async function bulkGame(gameId: string) {
  await setMeta([...selected.value], gameId || null, undefined);
}

async function bulkAuthor(author: string) {
  await setMeta([...selected.value], undefined, author || null);
}

async function deleteSelected() {
  const names = [...selected.value];
  if (!names.length) return;
  busy.value = true;
  try {
    await invoke("delete_inbox", { names });
    clearSelection();
    await refreshInbox();
    status.value = `Deleted ${names.length} item(s)`;
  } catch (e) {
    status.value = `Delete failed: ${e}`;
  } finally {
    busy.value = false;
  }
}

async function processNames(names: string[], passwords: Record<string, string> = {}) {
  if (!names.length) return;
  busy.value = true;
  processAlert.value = "";
  status.value = `Processing ${names.length} item(s)...`;
  try {
    const result = await invoke<ProcessInboxResult>("process_inbox", {
      names,
      passwords,
    });
    const needPw = result.results.filter((r) => r.needsPassword).map((r) => r.name);
    const failed = result.results.filter((r) => !r.success && !r.needsPassword);
    const ok = result.results.filter((r) => r.success);
    const alertText = result.results
      .map((r) => (r.message || "").trim())
      .filter(Boolean)
      .join("\n");
    if (alertText) processAlert.value = alertText;

    if (needPw.length) {
      passwordFor.value = needPw;
      passwordValue.value = "";
      passwordModal.value = true;
      status.value = `${ok.length} ok, password needed for ${needPw.length}`;
    } else if (failed.length) {
      status.value = `${ok.length} ok, ${failed.length} failed: ${failed
        .map((f) => `${f.name} (${JSON.stringify(f.error)})`)
        .join("; ")}`;
    } else {
      status.value = `Processed ${ok.length} item(s), placed ${result.installedMods.length} mod(s)`;
    }
    clearSelection();
    await refreshInbox();
  } catch (e) {
    status.value = `Process failed: ${e}`;
  } finally {
    busy.value = false;
  }
}

async function processSelected() {
  await processNames([...selected.value]);
}

async function processReady() {
  await processNames(items.value.filter((i) => i.ready).map((i) => i.name));
}

async function submitPassword() {
  const pw = passwordValue.value;
  const names = [...passwordFor.value];
  passwordModal.value = false;
  const map: Record<string, string> = {};
  for (const n of names) map[n] = pw;
  await processNames(names, map);
}

async function openIncoming() {
  busy.value = true;
  try {
    await invoke<string>("open_incoming_folder");
    status.value = "Opened Incoming folder";
  } catch (e) {
    status.value = `Open Incoming failed: ${e}`;
  } finally {
    busy.value = false;
  }
}

function newGameId(name: string) {
  return (
    name
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-|-$/g, "") || `game-${Date.now()}`
  );
}

function openAddGameModal() {
  addGameName.value = "";
  addGameModal.value = true;
}

async function confirmAddGame() {
  if (!config.value) return;
  const name = addGameName.value.trim();
  if (!name) return;
  const id = newGameId(name);
  config.value.games.push({
    id,
    name,
    modFolder: "",
    organizeByAuthor: true,
    authors: [],
  });
  addGameModal.value = false;
  expandedGames.value = new Set([...expandedGames.value, id]);
  await persistConfig();
  await refreshAllAuthorLogic();
}

function askRemoveGame(id: string, name: string) {
  if (!config.value) return;
  removeGameTarget.value = { id, name };
  removeGameModal.value = true;
}

async function confirmRemoveGame() {
  if (!config.value || !removeGameTarget.value) return;
  const id = removeGameTarget.value.id;
  config.value.games = config.value.games.filter((g) => g.id !== id);
  const next = new Set(expandedGames.value);
  next.delete(id);
  expandedGames.value = next;
  removeGameModal.value = false;
  removeGameTarget.value = null;
  await persistConfig();
}

async function pickModFolder(game: GameConfig) {
  const dir = await open({ directory: true, multiple: false });
  if (!dir || Array.isArray(dir)) return;
  game.modFolder = dir;
  await persistConfig();
}

async function pickIncoming() {
  if (!config.value) return;
  const dir = await open({ directory: true, multiple: false });
  if (!dir || Array.isArray(dir)) return;
  config.value.incomingDirectory = dir;
  await persistConfig();
}

function fieldValue(author: GameAuthorSettings, id: string) {
  ensureAuthorShape(author);
  return author.fieldValues[id] || "";
}

async function setFieldValue(author: GameAuthorSettings, id: string, value: string) {
  ensureAuthorShape(author);
  author.fieldValues = { ...author.fieldValues, [id]: value };
  await persistConfig();
}

async function pickFieldFile(author: GameAuthorSettings, field: LogicFieldSpec) {
  const exts = (field.accept || [])
    .map((a) => a.replace(/^\./, ""))
    .filter((a) => a && !a.includes("/"));
  const file = await open({
    multiple: false,
    title: field.label,
    filters: exts.length
      ? [
          { name: field.label, extensions: exts },
          { name: "All files", extensions: ["*"] },
        ]
      : undefined,
  });
  if (!file || Array.isArray(file)) return;
  await setFieldValue(author, field.id, file);
}

async function clearFieldValue(author: GameAuthorSettings, id: string) {
  ensureAuthorShape(author);
  const next = { ...author.fieldValues };
  delete next[id];
  author.fieldValues = next;
  await persistConfig();
}

async function saveAuthorDisplayName(author: GameAuthorSettings, name: string) {
  const trimmed = name.trim();
  if (!trimmed) return;
  author.name = trimmed;
  if (!author.id) author.id = slugifyAuthorName(trimmed);
  await persistConfig();
}

function openAddAuthorModal(game: GameConfig) {
  addAuthorGame.value = game;
  addAuthorName.value = "";
  addAuthorModal.value = true;
}

async function confirmAddAuthor() {
  const game = addAuthorGame.value;
  const name = addAuthorName.value.trim();
  if (!game || !name) return;
  if (game.authors.some((a) => a.name.toLowerCase() === name.toLowerCase())) {
    status.value = "Author already exists on this game";
    return;
  }
  const author: GameAuthorSettings = {
    id: slugifyAuthorName(name) || `author-${Date.now()}`,
    name,
    fieldValues: {},
    customLogic: logicTemplate.value || "",
  };
  game.authors.push(author);
  addAuthorModal.value = false;
  addAuthorGame.value = null;
  addAuthorName.value = "";
  await persistConfig();
  await refreshAuthorLogic(author);
}

async function removeAuthorFromGame(game: GameConfig, authorId: string) {
  game.authors = game.authors.filter((a) => a.id !== authorId);
  const next = { ...authorLogicMap.value };
  delete next[authorId];
  authorLogicMap.value = next;
  await persistConfig();
}

async function openLogicEditor(author: GameAuthorSettings) {
  logicAuthor.value = author;
  logicDraft.value = author.customLogic?.trim()
    ? author.customLogic
    : logicTemplate.value || "";
  logicError.value = "";
  logicModal.value = true;
}

async function saveLogicEditor() {
  if (!logicAuthor.value) return;
  const result = await invoke<{ ok: boolean; error?: string | null }>("validate_mod_logic", {
    source: logicDraft.value,
  });
  if (!result.ok) {
    logicError.value = result.error || "Invalid script";
    return;
  }
  logicAuthor.value.customLogic = logicDraft.value;
  const author = logicAuthor.value;
  logicModal.value = false;
  logicAuthor.value = null;
  await persistConfig();
  await refreshAuthorLogic(author);
}

async function persistConfig() {
  if (!config.value) return;
  for (const g of config.value.games) {
    if (!Array.isArray(g.authors)) g.authors = [];
    for (const a of g.authors) ensureAuthorShape(a);
  }
  config.value = await invoke<AppConfig>("save_config", {
    partial: {
      games: config.value.games,
      incomingDirectory: config.value.incomingDirectory,
      lastGameId: null,
      lastAuthor: null,
    },
  });
  for (const g of config.value.games) {
    if (!Array.isArray(g.authors)) g.authors = [];
    for (const a of g.authors) ensureAuthorShape(a);
  }
  status.value = "Settings saved";
}

function formatSize(n: number) {
  if (!n) return "—";
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

function kindLabel(k: InboxItemKind) {
  return k.charAt(0).toUpperCase() + k.slice(1);
}

onMounted(async () => {
  await refreshAll();
  unlistenInbox = await listen("inbox-changed", async () => {
    await refreshInbox();
  });

  const webview = getCurrentWebview();
  unlistenDrop = await webview.onDragDropEvent(async (event) => {
    if (event.payload.type === "enter" || event.payload.type === "over") {
      dragging.value = true;
    } else if (event.payload.type === "leave") {
      dragging.value = false;
    } else if (event.payload.type === "drop") {
      dragging.value = false;
      await importPaths(event.payload.paths);
    }
  });
});

onUnmounted(() => {
  unlistenInbox?.();
  unlistenDrop?.();
});
</script>

<template>
  <div class="layout">
    <aside class="sidebar">
      <div class="brand">AutoAdd<span>NewMod</span></div>
      <nav class="nav">
        <button type="button" :class="{ active: tab === 'drop' }" @click="tab = 'drop'">Drop</button>
        <button type="button" :class="{ active: tab === 'inbox' }" @click="tab = 'inbox'">
          Inbox
          <span v-if="items.length" class="nav-count">{{ items.length }}</span>
        </button>
        <button type="button" :class="{ active: tab === 'settings' }" @click="tab = 'settings'">Settings</button>
      </nav>
      <div class="sidebar-status muted">{{ status }}</div>
    </aside>

    <main class="content">
      <section v-if="tab === 'drop'" class="panel drop-page">
        <div
          class="dropzone dropzone-lg"
          :class="{ active: dragging }"
          @dragover.prevent
          @drop.prevent
        >
          <strong>Drop mods to here</strong>
          <div class="row" style="justify-content: center; margin-top: 1.1rem">
            <button type="button" :disabled="busy" @click="onPickFiles">Browse files…</button>
            <button type="button" :disabled="busy" @click="onPickFolders">Browse folders…</button>
            <button type="button" :disabled="busy" @click="openIncoming">Open Incoming</button>
          </div>
        </div>
      </section>

      <section v-else-if="tab === 'inbox'" class="panel inbox-panel">
        <div class="mail-toolbar">
          <div class="mail-check" title="Select all" @click.stop="toggleSelectAll">
            <input
              type="checkbox"
              :checked="allSelected"
              :indeterminate.prop="someSelected"
              tabindex="-1"
              @click.prevent
            />
          </div>
          <button type="button" class="icon-btn" title="Refresh" :disabled="busy" @click="refreshInbox">
            <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true">
              <path fill="currentColor" d="M17.65 6.35A7.95 7.95 0 0 0 12 4a8 8 0 1 0 7.75 10h-2.1A6 6 0 1 1 12 6c1.66 0 3.14.69 4.22 1.78L13 11h7V4l-2.35 2.35z" />
            </svg>
          </button>

          <template v-if="selected.length">
            <select
              title="Set game"
              @change="bulkGame(($event.target as HTMLSelectElement).value); ($event.target as HTMLSelectElement).value = ''"
            >
              <option value="">Game…</option>
              <option v-for="g in config?.games || []" :key="g.id" :value="g.id">{{ g.name }}</option>
            </select>
            <select
              title="Set author"
              :disabled="!authorOptions.length"
              @change="bulkAuthor(($event.target as HTMLSelectElement).value); ($event.target as HTMLSelectElement).value = ''"
            >
              <option value="">Author…</option>
              <option v-for="a in authorOptions" :key="a.value" :value="a.value">{{ a.label }}</option>
            </select>
            <button type="button" class="icon-btn primary" title="Process selected" :disabled="busy" @click="processSelected">
              <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true">
                <path fill="currentColor" d="M8 5v14l11-7L8 5z" />
              </svg>
            </button>
            <button type="button" class="icon-btn danger-btn" title="Delete selected" :disabled="busy" @click="deleteSelected">
              <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true">
                <path fill="currentColor" d="M6 19c0 1.1.9 2 2 2h8c1.1 0 2-.9 2-2V7H6v12zM19 4h-3.5l-1-1h-5l-1 1H5v2h14V4z" />
              </svg>
            </button>
          </template>
          <template v-else>
            <button type="button" class="icon-btn primary" title="Process all ready" :disabled="busy" @click="processReady">
              <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true">
                <path fill="currentColor" d="M8 5v14l11-7L8 5z" />
              </svg>
            </button>
          </template>
        </div>

        <div v-if="!items.length" class="empty">Inbox is empty</div>

        <div v-else class="mail-list">
          <div
            v-for="item in items"
            :key="item.name"
            class="mail-row"
            :class="{ selected: isSelected(item.name), ready: item.ready }"
            @click="toggleSelect(item.name)"
          >
            <div class="mail-check">
              <input
                type="checkbox"
                :checked="isSelected(item.name)"
                tabindex="-1"
                @click.prevent
              />
            </div>

            <div class="mail-main">
              <div class="mail-line">
                <span class="mail-sender">{{ item.name }}</span>
                <span v-if="item.gameId" class="mail-badge">{{ gameName(item.gameId) }}</span>
                <span class="mail-snippet">{{ rowPreview(item) }}</span>
              </div>
              <div class="mail-assigns">
                <select
                  :value="item.gameId || ''"
                  title="Game"
                  @click.stop
                  @change="onGameChange(item.name, ($event.target as HTMLSelectElement).value)"
                >
                  <option value="">Game…</option>
                  <option v-for="g in config?.games || []" :key="g.id" :value="g.id">
                    {{ g.name }}
                  </option>
                </select>
                <select
                  :value="item.author || ''"
                  title="Author"
                  :disabled="!authorOptionsForGameId(item.gameId).length"
                  @click.stop
                  @change="onAuthorChange(item.name, ($event.target as HTMLSelectElement).value)"
                >
                  <option value="">Author…</option>
                  <option
                    v-for="a in authorOptionsForGameId(item.gameId)"
                    :key="a.value"
                    :value="a.value"
                  >
                    {{ a.label }}
                  </option>
                </select>
                <span v-if="item.ready" class="ok">Ready</span>
                <span v-else-if="!item.hasDecryptKey" class="warn">
                  Needs setup
                </span>
              </div>
            </div>
          </div>
        </div>
      </section>

      <section v-else class="panel settings-panel">
        <div class="settings-group settings-group-compact">
          <h2 class="settings-group-title">Incoming</h2>
          <div class="field field-tight">
            <label>Path</label>
            <div class="row">
              <input
                :value="config?.incomingDirectory || ''"
                style="flex: 1"
                @change="config && (config.incomingDirectory = ($event.target as HTMLInputElement).value); persistConfig()"
              />
              <button type="button" class="icon-btn" title="Browse Incoming folder" @click="pickIncoming">
                <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true">
                  <path fill="currentColor" d="M20 6h-8l-2-2H4c-1.1 0-2 .9-2 2v12c0 1.1.9 2 2 2h16c1.1 0 2-.9 2-2V8c0-1.1-.9-2-2-2zm0 12H4V8h16v10z" />
                </svg>
              </button>
            </div>
          </div>
        </div>

        <div class="settings-group">
          <div class="settings-group-header">
            <div>
              <h2 class="settings-group-title">Games</h2>
            </div>
            <button type="button" class="icon-btn primary" title="Add game" @click="openAddGameModal">
              <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true">
                <path fill="currentColor" d="M19 13h-6v6h-2v-6H5v-2h6V5h2v6h6v2z" />
              </svg>
            </button>
          </div>

          <div v-if="!(config?.games || []).length" class="empty" style="padding: 1.2rem">No games yet</div>

          <div
            v-for="g in config?.games || []"
            :key="g.id"
            class="game-card"
            :class="{ expanded: isGameExpanded(g.id) }"
          >
            <div class="game-card-header" @click="toggleGameExpanded(g.id)">
              <button
                type="button"
                class="icon-btn expand-btn"
                :aria-expanded="isGameExpanded(g.id)"
                :title="isGameExpanded(g.id) ? 'Collapse' : 'Expand'"
                @click.stop="toggleGameExpanded(g.id)"
              >
                <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true">
                  <path
                    fill="currentColor"
                    :d="
                      isGameExpanded(g.id)
                        ? 'M7.41 15.41 12 10.83l4.59 4.58L18 14l-6-6-6 6z'
                        : 'M7.41 8.59 12 13.17l4.59-4.58L18 10l-6 6-6-6z'
                    "
                  />
                </svg>
              </button>
              <div class="game-card-title">
                <strong>{{ g.name || 'Untitled game' }}</strong>
                <span class="muted">{{ g.id }}</span>
              </div>
              <button
                type="button"
                class="icon-btn danger-btn trash-btn"
                title="Remove game"
                @click.stop="askRemoveGame(g.id, g.name)"
              >
                <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true">
                  <path fill="currentColor" d="M6 19c0 1.1.9 2 2 2h8c1.1 0 2-.9 2-2V7H6v12zM19 4h-3.5l-1-1h-5l-1 1H5v2h14V4z" />
                </svg>
              </button>
            </div>

            <div v-show="isGameExpanded(g.id)" class="game-card-body">
              <div class="field">
                <label>Display name</label>
                <input v-model="g.name" @change="persistConfig" />
              </div>
              <div class="field">
                <label>Mod folder</label>
                <div class="row">
                  <input v-model="g.modFolder" style="flex: 1" @change="persistConfig" />
                  <button type="button" class="icon-btn" title="Browse mod folder" @click="pickModFolder(g)">
                    <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true">
                      <path fill="currentColor" d="M20 6h-8l-2-2H4c-1.1 0-2 .9-2 2v12c0 1.1.9 2 2 2h16c1.1 0 2-.9 2-2V8c0-1.1-.9-2-2-2zm0 12H4V8h16v10z" />
                    </svg>
                  </button>
                </div>
              </div>
              <label class="row" style="margin-bottom: 0.85rem">
                <input type="checkbox" v-model="g.organizeByAuthor" @change="persistConfig" />
                Organize into Author subfolders
              </label>

              <div class="authors-header">
                <h3 class="settings-subgroup-title">Authors</h3>
                <button type="button" class="icon-btn primary" title="Add author" @click="openAddAuthorModal(g)">
                  <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true">
                    <path fill="currentColor" d="M19 13h-6v6h-2v-6H5v-2h6V5h2v6h6v2z" />
                  </svg>
                </button>
              </div>
              <div v-if="!g.authors.length" class="muted" style="margin-bottom: 0.5rem; font-size: 0.85rem">
                No authors yet.
              </div>
              <div
                v-for="ga in g.authors"
                :key="ga.id"
                class="author-block"
              >
                <div class="author-block-head">
                  <strong>{{ ga.name }}</strong>
                  <div class="row" style="gap: 0.4rem">
                    <button
                      type="button"
                      class="icon-btn"
                      title="Custom logic"
                      @click="openLogicEditor(ga)"
                    >
                      <svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true">
                        <path fill="currentColor" d="M9.4 16.6 4.8 12l4.6-4.6L8 6l-6 6 6 6 1.4-1.4zm5.2 0 1.4 1.4 6-6-6-6-1.4 1.4 4.6 4.6-4.6 4.6z" />
                      </svg>
                    </button>
                    <button
                      type="button"
                      class="icon-btn danger-btn"
                      title="Remove author"
                      @click="removeAuthorFromGame(g, ga.id)"
                    >
                      <svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true">
                        <path fill="currentColor" d="M6 19c0 1.1.9 2 2 2h8c1.1 0 2-.9 2-2V7H6v12zM19 4h-3.5l-1-1h-5l-1 1H5v2h14V4z" />
                      </svg>
                    </button>
                  </div>
                </div>

                <div class="field">
                  <label>Author name</label>
                  <input
                    :value="ga.name"
                    @change="saveAuthorDisplayName(ga, ($event.target as HTMLInputElement).value)"
                  />
                </div>

                <div
                  v-for="field in fieldsForAuthor(ga)"
                  :key="field.id"
                  class="field"
                >
                  <label>
                    {{ field.label }}
                    <span v-if="field.required" class="warn">*</span>
                  </label>

                  <div v-if="field.kind === 'file'" class="row">
                    <input :value="fieldValue(ga, field.id)" style="flex: 1" readonly />
                    <button type="button" class="icon-btn" title="Browse" @click="pickFieldFile(ga, field)">
                      <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true">
                        <path fill="currentColor" d="M14 2H6c-1.1 0-2 .9-2 2v16c0 1.1.9 2 2 2h12c1.1 0 2-.9 2-2V8l-6-6zm4 18H6V4h7v5h5v11z" />
                      </svg>
                    </button>
                    <button type="button" class="icon-btn" title="Clear" @click="clearFieldValue(ga, field.id)">
                      <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true">
                        <path fill="currentColor" d="M19 6.41 17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12z" />
                      </svg>
                    </button>
                  </div>

                  <input
                    v-else-if="field.kind === 'password' || field.secret"
                    type="password"
                    :value="fieldValue(ga, field.id)"
                    @change="setFieldValue(ga, field.id, ($event.target as HTMLInputElement).value)"
                  />

                  <textarea
                    v-else-if="field.kind === 'lines'"
                    rows="3"
                    :value="fieldValue(ga, field.id)"
                    @change="setFieldValue(ga, field.id, ($event.target as HTMLTextAreaElement).value)"
                  />

                  <input
                    v-else
                    type="text"
                    :value="fieldValue(ga, field.id)"
                    @change="setFieldValue(ga, field.id, ($event.target as HTMLInputElement).value)"
                  />
                </div>
              </div>
            </div>
          </div>
        </div>
      </section>
    </main>

    <div v-if="passwordModal" class="modal-backdrop" @click.self="passwordModal = false">
      <div class="modal" role="dialog" aria-modal="true">
        <h3 style="margin-top: 0">Archive password required</h3>
        <p class="muted">Needed for: {{ passwordFor.join(", ") }}</p>
        <div class="field">
          <label>Password</label>
          <input v-model="passwordValue" type="password" @keyup.enter="submitPassword" />
        </div>
        <div class="row" style="justify-content: flex-end">
          <button type="button" @click="passwordModal = false">Cancel</button>
          <button type="button" class="primary" @click="submitPassword">Retry</button>
        </div>
      </div>
    </div>

    <div v-if="addGameModal" class="modal-backdrop" @click.self="addGameModal = false">
      <div class="modal" role="dialog" aria-modal="true">
        <h3 style="margin-top: 0">Add game</h3>
        <div class="field">
          <label>Game name</label>
          <input
            v-model="addGameName"
            placeholder="e.g. My game"
            autofocus
            @keyup.enter="confirmAddGame"
          />
        </div>
        <div class="row" style="justify-content: flex-end">
          <button type="button" @click="addGameModal = false">Cancel</button>
          <button type="button" class="primary" :disabled="!addGameName.trim()" @click="confirmAddGame">Add</button>
        </div>
      </div>
    </div>

    <div v-if="removeGameModal" class="modal-backdrop" @click.self="removeGameModal = false">
      <div class="modal" role="dialog" aria-modal="true">
        <h3 style="margin-top: 0">Remove game?</h3>
        <p class="muted">
          Remove <strong>{{ removeGameTarget?.name }}</strong> from settings?
        </p>
        <div class="row" style="justify-content: flex-end">
          <button type="button" @click="removeGameModal = false">Cancel</button>
          <button type="button" class="danger-solid" @click="confirmRemoveGame">Remove</button>
        </div>
      </div>
    </div>

    <div v-if="addAuthorModal" class="modal-backdrop" @click.self="addAuthorModal = false">
      <div class="modal" role="dialog" aria-modal="true">
        <h3 style="margin-top: 0">Add author</h3>
        <div class="field">
          <label>Author name</label>
          <input
            v-model="addAuthorName"
            placeholder="e.g. Mod author"
            autofocus
            @keyup.enter="confirmAddAuthor"
          />
        </div>
        <div class="row" style="justify-content: flex-end">
          <button type="button" @click="addAuthorModal = false">Cancel</button>
          <button type="button" class="primary" :disabled="!addAuthorName.trim()" @click="confirmAddAuthor">
            Add
          </button>
        </div>
      </div>
    </div>

    <div v-if="logicModal" class="modal-backdrop" @click.self="logicModal = false">
      <div class="modal modal-wide" role="dialog" aria-modal="true">
        <h3 style="margin-top: 0">{{ logicAuthor?.name }} — logic</h3>
        <p class="logic-help">
          Python-like script. Built-ins: <code>mod</code>, <code>message</code>, <code>type</code>.
          Use <code>field</code>, <code>if</code>/<code>elif</code>/<code>else</code>, <code>var</code>,
          and <code>return true</code>/<code>false</code>. Set <code>message</code> to show a bottom alert.
        </p>
        <LogicEditor v-model="logicDraft" />
        <p v-if="logicError" class="warn" style="margin: 0.6rem 0">{{ logicError }}</p>
        <div class="row" style="justify-content: flex-end; margin-top: 0.75rem">
          <button type="button" @click="logicModal = false">Cancel</button>
          <button type="button" class="primary" @click="saveLogicEditor">Save</button>
        </div>
      </div>
    </div>

    <div v-if="processAlert" class="process-alert" role="status">
      <span>{{ processAlert }}</span>
      <button type="button" class="icon-btn" title="Dismiss" @click="processAlert = ''">
        <svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true">
          <path fill="currentColor" d="M19 6.41 17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12z" />
        </svg>
      </button>
    </div>
  </div>
</template>
