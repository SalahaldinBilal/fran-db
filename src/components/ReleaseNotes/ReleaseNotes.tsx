import styles from "./ReleaseNotes.module.scss";
import { createMemo } from "solid-js";
import { marked } from "marked";
import DOMPurify from "dompurify";

function ReleaseNotes(props: { notes: string }) {
  // latest.json isn't signed, so the notes are untrusted HTML. Links keep their text so a click can't navigate the app window.
  const html = createMemo(() =>
    DOMPurify.sanitize(marked.parse(props.notes, { async: false }), { FORBID_TAGS: ["a"] }),
  );

  return <div class={styles.ReleaseNotes} innerHTML={html()} />;
}

export default ReleaseNotes;
