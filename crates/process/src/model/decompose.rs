// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Top-down decomposition of separable nets into POWL.
//!
//! Provenance: Algorithm shape follows ex4pm `wf_net_to_powl.ex` (PETRI25
//! Algorithms 1-3: PartitionMG/PartitionSM) with smalt `Powl2.Compiler`'s
//! discipline: a net that is neither a marked graph net nor a state-machine
//! net is refused, not silently mis-projected.

use crate::error::ProcessError;
use crate::model::petri::PetriNet;
use crate::model::powl::{Endpoint, Powl};

/// Decompose a separable net into a POWL node.
///
/// - One transition: its [`Powl::Activity`] (or [`Powl::Silent`] when the
///   label is absent).
/// - Marked graph: [`Powl::PartialOrder`] with precedence edges derived per
///   place from input-to-output transition pairs (Algorithm 1).
/// - State machine: [`Powl::ChoiceGraph`] wired through the delimiters per
///   source/sink place (Algorithm 2), validated via [`Powl::choice_graph`].
/// - Anything else is refused ([`ProcessError::InvalidWorkflowNet`]) where
///   ex4pm silently mis-projected.
///
/// # Errors
/// [`ProcessError::InvalidWorkflowNet`] or [`ProcessError::InvalidChoiceGraph`].
pub fn decompose(net: &PetriNet) -> Result<Powl, ProcessError> {
    match net.transitions().len() {
        0 => Err(ProcessError::InvalidWorkflowNet {
            reason: "cannot decompose a net with no transitions",
        }),
        1 => {
            let transition = &net.transitions()[0];
            match &transition.label {
                Some(label) => Ok(Powl::activity(label.clone(), label.clone())),
                None => Ok(Powl::silent("tau")),
            }
        }
        _ => {
            if marked_graph(net) {
                partition_mg(net)
            } else if state_machine(net) {
                partition_sm(net)
            } else {
                Err(ProcessError::InvalidWorkflowNet {
                    reason: "net is neither marked graph nor state machine; recursive SESE \
                             decomposition is deferred",
                })
            }
        }
    }
}

/// Every place has at most one input and at most one output transition.
fn marked_graph(net: &PetriNet) -> bool {
    let places = net.places();
    let mut in_degree = vec![0usize; places];
    let mut out_degree = vec![0usize; places];
    for transition in net.transitions() {
        // A transition's inputs are the place's outgoing flow; its outputs
        // are the place's incoming flow.
        for (place, _) in &transition.inputs {
            out_degree[*place] += 1;
        }
        for (place, _) in &transition.outputs {
            in_degree[*place] += 1;
        }
    }
    in_degree
        .iter()
        .chain(out_degree.iter())
        .all(|count| *count <= 1)
}

/// Every transition has exactly one input place and exactly one output place,
/// both with unit weight.
fn state_machine(net: &PetriNet) -> bool {
    net.transitions().iter().all(|t| {
        t.inputs.len() == 1 && t.outputs.len() == 1 && t.inputs[0].1 == 1 && t.outputs[0].1 == 1
    })
}

/// Algorithm 1 (PartitionMG): poset edges from place-wise input-to-output
/// transition pairs, children in transition index order.
fn partition_mg(net: &PetriNet) -> Result<Powl, ProcessError> {
    let children: Vec<Powl> = net
        .transitions()
        .iter()
        .enumerate()
        .map(|(index, t)| match &t.label {
            Some(label) => Powl::activity(label.clone(), label.clone()),
            None => Powl::silent(format!("t{index}")),
        })
        .collect();
    let mut order: Vec<(usize, usize)> = Vec::new();
    for place in 0..net.places() {
        let ins = arcs_at(net, place, true);
        let outs = arcs_at(net, place, false);
        for u in &ins {
            for v in &outs {
                if u != v {
                    order.push((*u, *v));
                }
            }
        }
    }
    order.sort();
    order.dedup();
    Powl::partial_order("mg_root", children, order)
}

/// Algorithm 2 (PartitionSM): choice-graph edges per place, with the source
/// place wired to the start delimiter and the sink place to the end
/// delimiter.
fn partition_sm(net: &PetriNet) -> Result<Powl, ProcessError> {
    let facts = net.validate_workflow_net()?;
    let places = net.places();
    let children: Vec<Powl> = net
        .transitions()
        .iter()
        .enumerate()
        .map(|(index, t)| match &t.label {
            Some(label) => Powl::activity(label.clone(), label.clone()),
            None => Powl::silent(format!("t{index}")),
        })
        .collect();
    let mut edges: Vec<(Endpoint, Endpoint)> = Vec::new();
    for place in 0..places {
        let ins = arcs_at(net, place, true);
        let outs = arcs_at(net, place, false);
        if place == facts.source {
            for v in &outs {
                edges.push((Endpoint::Start, Endpoint::Child(*v)));
            }
        } else if place == facts.sink {
            for u in &ins {
                edges.push((Endpoint::Child(*u), Endpoint::End));
            }
        } else {
            for u in &ins {
                for v in &outs {
                    edges.push((Endpoint::Child(*u), Endpoint::Child(*v)));
                }
            }
        }
    }
    edges.sort();
    edges.dedup();
    Powl::choice_graph("cg_root", children, edges)
}

/// Transitions on one side of `place`: the transitions feeding it (whose
/// outputs produce into it) when `feeding`, else the transitions consuming
/// from it. ex4pm's `get_input_transitions` = feeding side.
fn arcs_at(net: &PetriNet, place: usize, feeding: bool) -> Vec<usize> {
    let mut out: Vec<usize> = net
        .transitions()
        .iter()
        .enumerate()
        .filter(|(_, t)| {
            let side = if feeding { &t.outputs } else { &t.inputs };
            side.iter().any(|(p, _)| *p == place)
        })
        .map(|(index, _)| index)
        .collect();
    out.sort_unstable();
    out
}
