use crate::{Element, ModelFile};

/// A model being edited: the file contents plus selection and undo history.
#[derive(Debug)]
pub struct ModelDocument {
    model: ModelFile,
    undo: Vec<ModelFile>,
    redo: Vec<ModelFile>,
    revision: u64,
    /// Key of the edit that further edits with the same key are merged into.
    coalescing: Option<u64>,
    selected: Option<usize>,
}

impl ModelDocument {
    pub fn new(model: ModelFile) -> Self {
        let selected = (!model.elements().is_empty()).then_some(0);
        Self {
            model,
            undo: Vec::new(),
            redo: Vec::new(),
            revision: 0,
            coalescing: None,
            selected,
        }
    }

    pub fn model(&self) -> &ModelFile {
        &self.model
    }

    /// Changes whenever the model's contents change.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub fn selected_element(&self) -> Option<&Element> {
        self.model.elements().get(self.selected?)
    }

    pub fn select(&mut self, index: Option<usize>) {
        self.selected = index.filter(|i| *i < self.model.elements().len());
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Applies an edit as one undo step. Edits that change nothing are not recorded.
    pub fn modify(&mut self, edit: impl FnOnce(&mut ModelFile)) {
        self.modify_keyed(None, edit);
    }

    /// Like [`Self::modify`], but consecutive edits with the same key (e.g. the frames of one
    /// slider drag) are merged into a single undo step until [`Self::end_coalescing`].
    pub fn modify_coalesced(&mut self, key: u64, edit: impl FnOnce(&mut ModelFile)) {
        self.modify_keyed(Some(key), edit);
    }

    fn modify_keyed(&mut self, key: Option<u64>, edit: impl FnOnce(&mut ModelFile)) {
        let before = self.model.clone();
        edit(&mut self.model);
        if self.model == before {
            return;
        }
        let merge = key.is_some() && key == self.coalescing && !self.undo.is_empty();
        if !merge {
            self.undo.push(before);
        }
        self.coalescing = key;
        self.redo.clear();
        self.revision += 1;
        self.fix_selection();
    }

    /// Call when an interaction ends so the next edit starts a new undo step.
    pub fn end_coalescing(&mut self) {
        self.coalescing = None;
    }

    fn fix_selection(&mut self) {
        let count = self.model.elements().len();
        self.selected = match self.selected {
            _ if count == 0 => None,
            Some(i) => Some(i.min(count - 1)),
            None => None,
        };
    }

    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.undo.pop() else {
            return false;
        };
        self.redo.push(std::mem::replace(&mut self.model, previous));
        self.after_history();
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };
        self.undo.push(std::mem::replace(&mut self.model, next));
        self.after_history();
        true
    }

    fn after_history(&mut self) {
        self.coalescing = None;
        self.revision += 1;
        self.fix_selection();
    }

    /// Adds a cube after the selected element (or at the end) and selects it.
    pub fn add_element(&mut self, element: Element) {
        let at = self.selected.map_or(self.model.elements().len(), |i| i + 1);
        self.modify(|model| model.elements.get_or_insert_default().insert(at, element));
        self.selected = Some(at);
    }

    pub fn duplicate_selected(&mut self) {
        if let Some(element) = self.selected_element().cloned() {
            self.add_element(element);
        }
    }

    pub fn remove_selected(&mut self) {
        let Some(index) = self.selected else { return };
        self.modify(|model| {
            if let Some(elements) = &mut model.elements
                && index < elements.len()
            {
                elements.remove(index);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube(size: f32) -> Element {
        Element::cube([0.0; 3], [size; 3], "#0")
    }

    #[test]
    fn add_duplicate_remove_and_selection() {
        let mut doc = ModelDocument::new(ModelFile::default());
        assert_eq!(doc.selected(), None);
        doc.add_element(cube(1.0));
        doc.add_element(cube(2.0));
        assert_eq!((doc.model().elements().len(), doc.selected()), (2, Some(1)));

        doc.select(Some(0));
        doc.duplicate_selected();
        let sizes: Vec<f32> = doc.model().elements().iter().map(|e| e.to[0]).collect();
        assert_eq!(sizes, [1.0, 1.0, 2.0]);
        assert_eq!(doc.selected(), Some(1));

        doc.select(Some(2));
        doc.remove_selected();
        assert_eq!(doc.selected(), Some(1), "selection stays in range");
        doc.select(Some(9));
        assert_eq!(doc.selected(), None);
    }

    #[test]
    fn undo_redo() {
        let mut doc = ModelDocument::new(ModelFile::default());
        doc.add_element(cube(1.0));
        doc.modify(|m| m.elements.as_mut().unwrap()[0].to = [5.0; 3]);
        let revision = doc.revision();
        doc.modify(|_| {});
        assert_eq!(doc.revision(), revision, "no-op edits are not recorded");

        assert!(doc.undo());
        assert_eq!(doc.model().elements()[0].to, [1.0; 3]);
        assert!(doc.undo());
        assert!(doc.model().elements().is_empty());
        assert_eq!(doc.selected(), None);
        assert!(!doc.undo());
        assert!(doc.redo() && doc.redo() && !doc.redo());
        assert_eq!(doc.model().elements()[0].to, [5.0; 3]);

        doc.undo();
        doc.modify(|m| m.parent = Some("block/cube".into()));
        assert!(!doc.can_redo());
    }

    #[test]
    fn coalesced_edits_form_one_undo_step() {
        let mut doc = ModelDocument::new(ModelFile::default());
        doc.add_element(cube(1.0));
        for size in [2.0, 3.0, 4.0] {
            doc.modify_coalesced(7, |m| m.elements.as_mut().unwrap()[0].to = [size; 3]);
        }
        doc.undo();
        assert_eq!(doc.model().elements()[0].to, [1.0; 3]);
        doc.redo();

        // After the interaction ends, the same key starts a new step.
        doc.end_coalescing();
        doc.modify_coalesced(7, |m| m.elements.as_mut().unwrap()[0].to = [9.0; 3]);
        doc.undo();
        assert_eq!(doc.model().elements()[0].to, [4.0; 3]);

        // A different key never merges.
        doc.modify_coalesced(1, |m| m.elements.as_mut().unwrap()[0].from = [1.0; 3]);
        doc.modify_coalesced(2, |m| m.elements.as_mut().unwrap()[0].from = [2.0; 3]);
        doc.undo();
        assert_eq!(doc.model().elements()[0].from, [1.0; 3]);
    }
}
