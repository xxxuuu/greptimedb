// Copyright 2023 Greptime Team
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::rc::Rc;

use hydroflow::scheduled::graph::Hydroflow;
use hydroflow::scheduled::SubgraphId;

use crate::compute::types::ErrCollector;
use crate::repr::{self, Timestamp};

/// input/output of a dataflow
/// One `ComputeState` manage the input/output/schedule of one `Hydroflow`
#[derive(Default)]
pub struct DataflowState {
    /// it is important to use a deque to maintain the order of subgraph here
    /// TODO(discord9): consider dedup? Also not necessary for hydroflow itself also do dedup when schedule
    schedule_subgraph: Rc<RefCell<BTreeMap<Timestamp, VecDeque<SubgraphId>>>>,
    /// Frontier (in sys time) before which updates should not be emitted.
    ///
    /// We *must* apply it to sinks, to ensure correct outputs.
    /// We *should* apply it to sources and imported shared state, because it improves performance.
    /// Which means it's also the current time in temporal filter to get current correct result
    as_of: Rc<RefCell<Timestamp>>,
    /// error collector local to this `ComputeState`,
    /// useful for distinguishing errors from different `Hydroflow`
    err_collector: ErrCollector,
}

impl DataflowState {
    /// schedule all subgraph that need to run with time <= `as_of` and run_available()
    ///
    /// return true if any subgraph actually executed
    pub fn run_available_with_schedule(&mut self, df: &mut Hydroflow) -> bool {
        // first split keys <= as_of into another map
        let mut before = self
            .schedule_subgraph
            .borrow_mut()
            .split_off(&(*self.as_of.borrow() + 1));
        std::mem::swap(&mut before, &mut self.schedule_subgraph.borrow_mut());
        for (_, v) in before {
            for subgraph in v {
                df.schedule_subgraph(subgraph);
            }
        }
        df.run_available()
    }
    pub fn get_scheduler(&self) -> Scheduler {
        Scheduler {
            schedule_subgraph: self.schedule_subgraph.clone(),
            cur_subgraph: Rc::new(RefCell::new(None)),
        }
    }

    /// return a handle to the current time, will update when `as_of` is updated
    ///
    /// so it can keep track of the current time even in a closure that is called later
    pub fn current_time_ref(&self) -> Rc<RefCell<Timestamp>> {
        self.as_of.clone()
    }

    pub fn current_ts(&self) -> Timestamp {
        *self.as_of.borrow()
    }

    pub fn set_current_ts(&mut self, ts: Timestamp) {
        self.as_of.replace(ts);
    }

    pub fn get_err_collector(&self) -> ErrCollector {
        self.err_collector.clone()
    }
}

#[derive(Clone)]
pub struct Scheduler {
    schedule_subgraph: Rc<RefCell<BTreeMap<Timestamp, VecDeque<SubgraphId>>>>,
    cur_subgraph: Rc<RefCell<Option<SubgraphId>>>,
}

impl Scheduler {
    pub fn schedule_at(&self, next_run_time: Timestamp) {
        let mut schedule_subgraph = self.schedule_subgraph.borrow_mut();
        let subgraph = self.cur_subgraph.borrow();
        let subgraph = subgraph.as_ref().expect("Set SubgraphId before schedule");
        let subgraph_queue = schedule_subgraph.entry(next_run_time).or_default();
        subgraph_queue.push_back(*subgraph);
    }

    pub fn set_cur_subgraph(&self, subgraph: SubgraphId) {
        self.cur_subgraph.replace(Some(subgraph));
    }
}
