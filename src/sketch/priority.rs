use std::cmp::Ordering;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriorityItem<T> {
    pub priority: u64,
    pub value: T,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RankedPriorityItem<T> {
    pub rank: u32,
    pub priority: u64,
    pub value: T,
}

#[derive(Debug, Clone)]
pub struct PrioritySampler<T> {
    limit: usize,
    items: Vec<PriorityItem<T>>,
}

impl<T> PrioritySampler<T>
where
    T: Clone,
{
    pub fn new(limit: usize) -> Self {
        Self {
            limit,
            items: Vec::with_capacity(limit),
        }
    }

    pub fn push(&mut self, priority: u64, value: T) {
        if self.limit == 0 {
            return;
        }

        let item = PriorityItem { priority, value };
        if self.items.len() < self.limit {
            self.items.push(item);
            return;
        }

        let Some((worst_index, worst_item)) = self
            .items
            .iter()
            .enumerate()
            .max_by(|(_, left), (_, right)| compare_priority(left, right))
        else {
            return;
        };

        if compare_priority(&item, worst_item).is_lt() {
            self.items[worst_index] = item;
        }
    }

    pub fn ranked(&self) -> Vec<RankedPriorityItem<T>> {
        let mut items = self.items.clone();
        items.sort_by(compare_priority);
        items
            .into_iter()
            .enumerate()
            .map(|(index, item)| RankedPriorityItem {
                rank: (index + 1) as u32,
                priority: item.priority,
                value: item.value,
            })
            .collect()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }
}

fn compare_priority<T>(left: &PriorityItem<T>, right: &PriorityItem<T>) -> Ordering {
    left.priority.cmp(&right.priority)
}
