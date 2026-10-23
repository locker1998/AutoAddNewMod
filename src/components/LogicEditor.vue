<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from "vue";
import { EditorView, basicSetup } from "codemirror";
import { EditorState } from "@codemirror/state";
import { HighlightStyle, indentUnit, StreamLanguage, syntaxHighlighting } from "@codemirror/language";
import { tags as t } from "@lezer/highlight";
import { keymap } from "@codemirror/view";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";

const props = defineProps<{ modelValue: string }>();
const emit = defineEmits<{ "update:modelValue": [string] }>();

const host = ref<HTMLElement | null>(null);
let view: EditorView | null = null;

const modLogicLanguage = StreamLanguage.define({
  startState() {
    return {};
  },
  token(stream) {
    if (stream.sol() && stream.match(/#.*/)) return "comment";
    if (stream.match(/"(?:[^"\\]|\\.)*"/) || stream.match(/'(?:[^'\\]|\\.)*'/)) {
      return "string";
    }
    if (stream.match(/\b(?:field|if|elif|else|var|return|true|false)\b/)) {
      return "keyword";
    }
    if (stream.match(/\b(?:mod|type|message)\b/)) {
      return "atom";
    }
    if (
      stream.match(
        /\b(?:file|folder|archive|as_file|as_text|as_textarea|as_password|accept|hide|required|extract|split|get_type)\b/
      )
    ) {
      return "atom";
    }
    if (stream.match(/\.[A-Za-z0-9_]+/)) return "attributeName";
    if (stream.match(/[A-Za-z_][\w-]*/)) return "variableName";
    stream.next();
    return null;
  },
});

const highlight = HighlightStyle.define([
  { tag: t.comment, color: "#6b7280", fontStyle: "italic" },
  { tag: t.keyword, color: "#7aa2f7", fontWeight: "600" },
  { tag: t.atom, color: "#9ece6a" },
  { tag: t.string, color: "#e0af68" },
  { tag: t.attributeName, color: "#bb9af7" },
  { tag: t.variableName, color: "#c0caf5" },
]);

const theme = EditorView.theme(
  {
    "&": {
      backgroundColor: "#0c0e14",
      color: "#eef1f7",
      fontSize: "13px",
      height: "100%",
    },
    ".cm-content": {
      fontFamily: 'Consolas, "Cascadia Mono", monospace',
      caretColor: "#eef1f7",
      padding: "0.6rem 0",
    },
    ".cm-gutters": {
      backgroundColor: "#0c0e14",
      color: "#5b6478",
      border: "none",
    },
    ".cm-activeLine": { backgroundColor: "rgba(255,255,255,0.03)" },
    ".cm-activeLineGutter": { backgroundColor: "rgba(255,255,255,0.03)" },
    "&.cm-focused .cm-cursor": { borderLeftColor: "#8eb0ff" },
    "&.cm-focused .cm-selectionBackground, .cm-selectionBackground": {
      backgroundColor: "rgba(91, 141, 239, 0.28)",
    },
    ".cm-scroller": { overflow: "auto" },
  },
  { dark: true }
);

onMounted(() => {
  if (!host.value) return;
  view = new EditorView({
    state: EditorState.create({
      doc: props.modelValue || "",
      extensions: [
        basicSetup,
        history(),
        keymap.of([indentWithTab, ...defaultKeymap, ...historyKeymap]),
        indentUnit.of("\t"),
        modLogicLanguage,
        syntaxHighlighting(highlight),
        theme,
        EditorView.updateListener.of((update) => {
          if (update.docChanged) {
            emit("update:modelValue", update.state.doc.toString());
          }
        }),
      ],
    }),
    parent: host.value,
  });
});

watch(
  () => props.modelValue,
  (value) => {
    if (!view) return;
    const current = view.state.doc.toString();
    if (value !== current) {
      view.dispatch({
        changes: { from: 0, to: current.length, insert: value || "" },
      });
    }
  }
);

onBeforeUnmount(() => {
  view?.destroy();
  view = null;
});
</script>

<template>
  <div ref="host" class="logic-ide" />
</template>

<style scoped>
.logic-ide {
  border: 1px solid #2a3142;
  border-radius: 10px;
  overflow: hidden;
  min-height: 22rem;
  height: 22rem;
}
</style>
