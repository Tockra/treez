///Implementation of sarsa Q-learning
///using eligibility trace update with replacement update,
///rollout factor, e-greedy or softmax policy selection

extern crate chrono;
extern crate rand;

use std::fmt;
use std::f64;
use std::hash;
use std::collections::HashMap;
use std::collections::HashSet;
use std::cmp;
use self::chrono::prelude::*;
//use self::rand::{Rng};
// use self::rand::distributions::{IndependentSample, Range};

use policy;
use softmax;

///Input search parameters
#[derive(Debug,Clone)]
pub struct SearchCriteria {
    ///lambda: rollout factor
    pub _lambda: f64,
    ///gamma: discount factor
    pub _gamma: f64,
    ///alpha: correction step size
    pub _alpha: f64,
    ///search stop condition
    pub _stop_limit: StopCondition,
    ///policy selection
    pub _policy_select_method: PolicySelectMethod,
}

impl SearchCriteria {
    pub fn check( & self ) -> Result< (), & 'static str > {
        if self._lambda < 0. || self._lambda > 1. ||
           self._gamma < 0. || self._gamma > 1. ||
           self._alpha <= 0.
        {
            Err( "search criteria out of range" )
        } else {
            match self._policy_select_method {
                PolicySelectMethod::EpsilonGreedy( x ) => { if x < 0. || x > 1.0 { return Err( "search criteria out of range" ) } },
                _ => {},
            }
            Ok( () )
        }
    }
}

#[derive(Debug, Clone)]
pub enum StopCondition {
    TimeMicro(f64), //time allotted to search
    EpisodeIter(usize), //max iterations allotted to search
}

#[derive(Debug, Clone)]
pub enum PolicySelectMethod {
    EpsilonGreedy( f64 ),
    Softmax,
}

pub struct Reward(pub f64);

///extensible interface to be defined by a specific game of interest
pub trait Game< State, Action > where State: Clone + cmp::Eq + hash::Hash, Action: Clone + cmp::Eq + hash::Hash {
    ///initial state to start with
    fn gen_initial_state( & mut self ) -> State;

    ///given state, give all possible actions
    fn gen_possible_actions( & mut self, & State ) -> Vec< Action >;

    ///select action at current state and transition to new state with reward
    fn do_action( & mut self, & State, & Action ) -> (Reward, State);

    fn is_state_terminal( & mut self, s: & State ) -> bool;

    fn get_state_history( & self ) -> Vec< ( State, Action ) >;

    fn set_state_history( & mut self, h: & [ (State, Action) ] );
}

///main entry point for search
pub fn search< G, State, Action >( criteria: & SearchCriteria,
                                   g: & mut G )
                                   -> Result< ( HashMap< ( State, Action ), f64 >,
                                                HashMap< State, Vec< ( Action, f64 ) > >,
                                                HashMap< State, f64 >,
                                                u64 ), & 'static str >
    where G: Game< State, Action >, State: Clone + cmp::Eq + hash::Hash + fmt::Debug, Action: Clone + cmp::Eq + hash::Hash + fmt::Debug {

    criteria.check()?;

    //init (state,action) -> value map
    let mut policy_values : HashMap< (State, Action), f64 > = HashMap::new();
                                   
    let t0 = Local::now();
    let mut iter = 0;

    //init state to some set value of interest
    let state_init = g.gen_initial_state(); //save this state to be reset at the start of an episode
    
    'outer_loop: loop { //per episode

        //init eligibility trace for state value estimation
        let mut eligibility_trace : HashMap< (State, Action), f64 > = HashMap::new();

        //reset state
        let mut state_episode = g.gen_initial_state();

        if g.is_state_terminal( & state_episode ) {
            break;
        }
        
        //init action
        let mut action : Action = {
            let possible_actions = g.gen_possible_actions( & state_init );
            
            match criteria._policy_select_method {
                PolicySelectMethod::EpsilonGreedy( epsilon ) => {
                    let action_greedy = policy::get_greedy_action_at_state( & policy_values, & state_episode );
                    policy::e_greedy_select( epsilon, possible_actions.as_slice(), & action_greedy )
                },
                PolicySelectMethod::Softmax => {
                    //obtain policy values for currently available actions at current state
                    let mut vals = softmax::Distr(vec![]);
                    for (_k,i) in possible_actions.iter().enumerate() {
                        let val = match policy_values.get( &( state_init.clone(), i.clone() ) ) {
                            Some( x ) => *x,
                            None => 0.,
                        };
                        vals.0.push(val);
                    }
                    policy::softmax_select( possible_actions.as_slice(), & vals )
                },
            }
        };

        loop { //per step in episode
            if g.is_state_terminal( & state_episode ) {
                break;
            }

            let ( reward, state_next ) = g.do_action( & state_episode, & action );
            //choose action using e-greedy policy selection
            let action_next : Action = {
                let possible_actions = g.gen_possible_actions( & state_next );

                match criteria._policy_select_method {
                    PolicySelectMethod::EpsilonGreedy( epsilon ) => {
                        let action_greedy = policy::get_greedy_action_at_state( & policy_values, & state_next );
                        policy::e_greedy_select( epsilon, possible_actions.as_slice(), & action_greedy )
                    },
                    PolicySelectMethod::Softmax => {
                        //obtain policy values for currently available actions at current state
                        let mut vals = softmax::Distr(vec![]);
                        for (_k,i) in possible_actions.iter().enumerate() {
                            // let val = policy_values.get( &( state_next.clone(), i.clone() ) ).unwrap_or( &0. );
                            // vals.0.push(*val);
                            let val = match policy_values.get( &( state_next.clone(), i.clone() ) ) {
                                Some( x ) => *x,
                                None => 0.,
                            };
                            vals.0.push(val);
                        }
                        policy::softmax_select( possible_actions.as_slice(), & vals )
                    },
                }
            };

            let td_error = {
                    
                let q_next = policy_values.get( &( state_next.clone(), action_next.clone() ) ).unwrap_or(&0.);
                let q = policy_values.get( &( state_episode.clone(), action.clone() ) ).unwrap_or(&0.);                
                reward.0 + criteria._gamma * q_next - q
            };

            //update eligibility trace
            {
                let eligibility = eligibility_trace.entry( ( state_episode.clone(), action.clone() ) ).or_insert( 0. );
                *eligibility = 1.;
            }

            //remove loops and zero out eligibility values for items in loops
            let mut loop_detector = HashMap::new();
            let mut items_in_path = HashSet::new();
            let mut items_in_loops = HashSet::new();
            let trace = g.get_state_history();
            
            for i in 0..trace.len() {
                let t =  & trace[i];
                let exists = match loop_detector.get( t ) {
                    None => { false },
                    Some(_) => { true },
                };
                if exists {
                    let index = *loop_detector.get( t ).unwrap();
                    for j in index..i {
                        items_in_path.remove( &j );
                        items_in_loops.insert( j );
                    }
                    loop_detector.insert( t.clone(), i );
                    items_in_path.insert( i );
                    items_in_loops.remove( &i );
                } else {
                    loop_detector.insert( t.clone(), i );
                    items_in_path.insert( i );
                    items_in_loops.remove( &i );
                }
            }

            for i in items_in_loops.iter() {
                let t = & trace[ *i ];                
                let v = eligibility_trace.get_mut( t ).unwrap();
                //eligibility trace decay
                *v = criteria._gamma * criteria._lambda * *v;
            }

            let normalized_policies = normalized_policy_actions( & policy_values );
            
            for i in items_in_path.iter() {
                let t = & trace[ *i ];

                //update policy value
                let qq = *policy_values.get( t ).unwrap_or(& 0.);
                
                let v = eligibility_trace.get_mut( t ).unwrap();

                let alpha_adjust = match normalized_policies.get( t ) {
                    Some(x) => { ( 1. - if x.is_nan() { 0. } else { *x } ) * criteria._alpha },
                    None => { criteria._alpha },
                };
                let n = qq + alpha_adjust * td_error * *v;
                
                policy_values.insert( ( t.0.clone(), t.1.clone() ), n );

                //eligibility trace decay
                *v = criteria._gamma * criteria._lambda * *v;
            }
            //filter out loops in trace history
            let mut sorted_index = items_in_path.iter().cloned().collect::<Vec<usize> >();
            sorted_index.sort();
            let history_filtered : Vec< (State, Action) > = sorted_index.iter().map( |x| trace[*x].clone() ).collect();
            g.set_state_history( history_filtered.as_slice() );

            //save state and action
            state_episode = state_next;
            action = action_next;

            //stopping condition check
            let t1 = Local::now();
            match criteria._stop_limit {
                StopCondition::TimeMicro(t) => {
                    let t_delta = t1.signed_duration_since(t0).num_microseconds().unwrap() as f64;
                    if t_delta >= t {
                        break 'outer_loop;
                    }
                },
                _ => {},
            }
        }
        //stopping condition check
        let t1 = Local::now();
        match criteria._stop_limit {
            StopCondition::TimeMicro(t) => {
                let t_delta = t1.signed_duration_since(t0).num_microseconds().unwrap() as f64;
                if t_delta >= t {
                    break;
                }
            },
            StopCondition::EpisodeIter(n) => {
                if iter >= n {
                    break;
                }
            },
        }
        iter += 1;
    }
    let policy_normalized = normalized_policy_actions_array( & policy_values );
    let expect = get_expectation_policy( & policy_values );
    Ok( ( policy_values, policy_normalized, expect, iter as u64 ) )
}

fn get_expectation_policy< State, Action >( policy_map: & HashMap< (State, Action), f64 > )
                                            -> HashMap< State, f64 >
    where State: Clone + cmp::Eq + hash::Hash + fmt::Debug,
          Action: Clone + cmp::Eq + hash::Hash + fmt::Debug {
    
    let mut h : HashMap< State, f64 > = HashMap::new();
    for i in policy_map.iter() {
        let x = h.entry( (i.0).0.clone() ).or_insert( 0. );
        *x += *i.1;
    }
    h
}

fn normalized_policy_actions< State, Action >( policy_map: & HashMap< (State, Action), f64 > ) -> HashMap< (State, Action), f64 >
    where State: Clone + cmp::Eq + hash::Hash + fmt::Debug,
          Action: Clone + cmp::Eq + hash::Hash + fmt::Debug {

    let mut h : HashMap< State, Vec< ( Action, f64 ) > > = HashMap::new();
    for i in policy_map.iter() {
        let v = h.entry( (i.0).0.clone() ).or_insert( vec![] );
        v.push( ( (i.0).1.clone(), *i.1 ) );
    }
    
    for i in h.iter_mut() {
        let bounds = i.1.iter()
            .fold( ( f64::MAX, f64::MIN ), |accum, x| {
                let mut b_l = accum.0;
                let mut b_h = accum.1;
                if x.1 <= b_l { b_l = x.1 }
                if x.1 >= b_h { b_h = x.1 }
                ( b_l, b_h )
            } );
        let total = i.1.iter().fold( 0., |accum, x| {
            accum + x.1 - bounds.0
        } );
        *i.1 = i.1.iter().cloned().map( |x| ( x.0, ( x.1 - bounds.0 ) / total ) ).collect();
    }
    let mut ret = HashMap::new();
    for i in h.iter() {
        for j in i.1.iter() {
            ret.insert( ( i.0.clone(), j.0.clone() ), j.1.clone() );
        }
    }
    ret
}


fn normalized_policy_actions_array< State, Action >( policy_map: & HashMap< (State, Action), f64 > ) -> HashMap< State, Vec< ( Action, f64 ) > >
    where State: Clone + cmp::Eq + hash::Hash + fmt::Debug,
          Action: Clone + cmp::Eq + hash::Hash + fmt::Debug {

    let mut h : HashMap< State, Vec< ( Action, f64 ) > > = HashMap::new();
    for i in policy_map.iter() {
        let v = h.entry( (i.0).0.clone() ).or_insert( vec![] );
        v.push( ( (i.0).1.clone(), *i.1 ) );
    }
    
    for i in h.iter_mut() {
        let bounds = i.1.iter()
            .fold( ( f64::MAX, f64::MIN ), |accum, x| {
                let mut b_l = accum.0;
                let mut b_h = accum.1;
                if x.1 <= b_l { b_l = x.1 }
                if x.1 >= b_h { b_h = x.1 }
                ( b_l, b_h )
            } );
        let total = i.1.iter().fold( 0., |accum, x| {
            accum + x.1 - bounds.0
        } );
        *i.1 = i.1.iter().cloned().map( |x| ( x.0, ( x.1 - bounds.0 ) / total ) ).collect();
    }
    h
}
