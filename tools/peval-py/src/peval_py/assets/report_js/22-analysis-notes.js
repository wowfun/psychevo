function renderSelectedNotes(trialKey) {
  const notes = notesFor(trialKey);
  const editor = renderNotesEditor(trialKey);
  const action = renderNotesAction(trialKey);
  const body = notes.length ? `<div class="note-list">${notes.map(renderManualNote).join("")}</div>` : `<p class="copy">${esc(t("no_notes", "No notes."))}</p>`;
  return `<section class="selected-extra selected-notes">
    <div class="selected-extra-head"><h3>${esc(t("notes", "Notes"))}</h3>${action}</div>
    ${editor}
    ${body}
  </section>`;
}
function renderNotesAction(trialKey) {
  const source = editableNotesSource(trialKey);
  if (!source || state.notesEditor?.trialKey === trialKey) return "";
  const cellNote = cellNoteFor(trialKey);
  const label = cellNote ? t("edit_notes", "Edit notes") : t("add_notes", "Add notes");
  return `<button class="step-toggle-button notes-edit-button" type="button" data-notes-edit data-trial-key="${esc(trialKey)}">${esc(label)}</button>`;
}
function renderNotesEditor(trialKey) {
  if (!serveMode() || !trialKey || !state.notesEditor || state.notesEditor.trialKey !== trialKey) return "";
  const markdown = state.notesEditor.markdown ?? "";
  const error = state.notesEditor.error ? `<p class="copy danger">${esc(state.notesEditor.error)}</p>` : "";
  const disabled = state.notesEditor.saving ? " disabled" : "";
  return `<article class="notes-editor-panel" data-notes-editor-panel>
    <textarea data-notes-editor data-trial-key="${esc(trialKey)}" rows="8">${esc(markdown)}</textarea>
    ${error}
    <div class="notes-editor-actions">
      <button class="step-toggle-button primary" type="button" data-notes-save data-trial-key="${esc(trialKey)}"${disabled}>${esc(t("save_notes", "Save notes"))}</button>
      <button class="step-toggle-button" type="button" data-notes-cancel${disabled}>${esc(t("cancel", "Cancel"))}</button>
    </div>
  </article>`;
}
function beginNotesEdit(trialKey) {
  if (!trialKey || !editableNotesSource(trialKey)) return;
  const note = cellNoteFor(trialKey);
  state.notesEditor = { trialKey, markdown: note?.markdown || "", error: "", saving: false };
  renderTrace();
}
function cancelNotesEdit() {
  state.notesEditor = null;
  renderTrace();
}
async function saveSelectedNotes(button) {
  const trialKey = button?.dataset?.trialKey || selectedKey();
  const source = editableNotesSource(trialKey);
  const panel = button?.closest?.("[data-notes-editor-panel]");
  const textarea = panel?.querySelector?.("[data-notes-editor]");
  if (!source?.source_key || !textarea) return;
  const markdown = textarea.value || "";
  state.notesEditor = { trialKey, markdown, error: "", saving: true };
  renderTrace();
  try {
    const payload = await serveApi(`/api/sources/${encodeURIComponent(source.source_key)}/notes`, {
      method: "POST",
      body: { markdown }
    });
    state.notesEditor = null;
    applyServeMutationPayload(payload, { preserveTrial: trialKey });
  } catch (error) {
    const message = `${t("notes_save_failed", "Save notes failed")}: ${error.message || String(error)}`;
    state.notesEditor = { trialKey, markdown, error: message, saving: false };
    setServeStatus(message, true);
    renderTrace();
  }
}
