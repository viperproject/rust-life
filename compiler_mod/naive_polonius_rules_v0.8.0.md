# Naive polonius borrowchecker rules, v0.8.0
There follows a list of all rules that are used by the polonius borrow checker (naive version), that were found in the file [polonius/polonius-engine/src/output/naive.rs](https://github.com/rust-lang/polonius/blob/master/polonius-engine/src/output/naive.rs). The rules are given as Datalog rules (using the Soufflé project syntax), basically they are just copies of the comments in the source file. In addition, we also try to give short explanations of all rules, these explanations are mostly based on the information in the blog post ["An alias-based formulation of the borrow checker"](http://smallcultfollowing.com/babysteps/blog/2018/04/27/an-alias-based-formulation-of-the-borrow-checker/) by Nicholas Matsakis, i.e. basically the first introduction of the polonius borrow checker.

This document gives the rules from version 0.4.0 of polonius, i.e. the ones from polonius-engine version 0.8.0.

Note that the construct that previously was called "loan", and often denoted as 'L' seems to now be called "borrow", and is now denoted as 'B'.

## Rules
First, these are the rules that are used by Polonius, where the last rule is defining an actual (eventual) error.

The part of the Subset relation that is given as an input fact that is called outlives. This is not computed by the polonius borrow checker, but given as static input.
```Datalog
subset(R1, R2, P) :-
  outlives(R1, R2, P).
```

Subset is transitive:
```Datalog
subset(R1, R3, P) :-
  subset(R1, R2, P),
  subset(R2, R3, P).
```

Propagates subset relationships across the control-flow graph edges:
```Datalog
subset(R1, R2, Q) :-
  subset(R1, R2, P),
  cfg_edge(P, Q),
  region_live_at(R1, Q),
  region_live_at(R2, Q).
```

Requires is described informally by the blog post by Nicholas Matsakis as follows:

> The region R requires the terms of the loan L to be enforced at the point P.
>
> Or, put another way:
>
> If the terms of the loan L are violated at the point P, then the region R is invalidated.

The first rule for requires says that the region for a borrow is always dependent on its corresponding loan:
```Datalog
requires(R, B, P) :-
  borrow_region(R, B, P).
```

The next rule says that if R1: R2, then R2 depends on any loans that R1 depends on:
```Datalog
requires(R2, B, P) :-
  requires(R1, B, P),
  subset(R1, R2, P).
```

This (basically) just propagates requires along cfg edges, but there is a twist. (The second-last line, giving !killed):
```Datalog
requires(R, B, Q) :-
  requires(R, B, P),
 !killed(B, P),
  cfg_edge(P, Q),
  region_live_at(R, Q).
```

A loan L (the same as a borrow B) is live at the point P if some live region R requires it:
```Datalog
borrow_live_at(B, P) :-
  requires(R, B, P),
  region_live_at(R, P).
```

Finally, it is an error if a point P invalidates a loan L while the loan L is live:
```Datalog
.decl errors(B, P) :-
  invalidates(B, P),
  borrow_live_at(B, P).
```

## Facts
As you might have noted, some of the relations that are used in the rules are not given by rules. As you might have guessed, these are so-called (input) facts. These are provided to Polonius as inputs from previous compilation phases. In principle, these are a representation of (parts) of the input program (input to the compiler), provided in a form that is well-suited for an analysis by Polonius and that especially points out the constraints that are implied by the input program and that must be satisfiable if the input shall be a valid Rust program.

This gives the outlive relationship, as the name already states. However, it actually seems to only give the part of the outlive relationshios that directly arise form the program code. The entire relationship is then defined by the `subset` rule. Therefore, this fact was called `base_subset` in the original blog post by Nicholas Matsakis.
```Datalog
.decl outlives(R1:Region, R2:Region, P:Point)
.input outlives
```

This fact is simply the control-flow graph (cfg) of the relevant program part, given as a set of edges that are connecting program points and thereby completely describe the graph.
```Datalog
.decl cfg_edge(P:Point, Q:Point)
.input cfg_edge
```

This basically gives the information that a certain region is live at a certain point. (The details of this, and the definitions are given in the blog post, that also redirects to the NLL RFC.)
However, note that in the current version of Polonius it does not directly use the `region_live_at` that is provided as input (as part of all_facts), but instad it uses a new one that is explicitly computed right at the beginning by Polonius by calling the method `liveness::init_region_live_at(...)`. (So the relation that is used by Polonius will differ from the one that is available as part of the inputs) Still, for the rules provided before, this (new) `region_live_at` is considered to be an input fact.
Also, it seems (from the commit history) that this change was only introduced in polonius-engine 0.8.0. Before, the `region_live_at` relation form the inputs was used directly.
```Datalog
.decl region_live_at(R:Region, P:Point)
.input region_live_at
```

Simply a quote of the description form the blog post by Nicholas Matsakis:

> This input is defined for each borrow expression (e.g., `&x` or `&mut v`) in the program. It relates the region from the borrow to the abstract loan that is created.
```Datalog
.decl borrow_region(R:Region, B:Borrow, P:Point)
.input borrow_region
```

Again, just a quote from the blog post by Nicholas Matsakis:

> `killed(L, P)` is defined when the point P is an assignment that overwrites one of the references whose referent was borrowed in the loan L.

For more details, pleas check the blog post that provided an illustrative example.
```Datalog
.decl killed(B:Borrow, P:Point)
.input killed
```

Finally, `invalidates` indicates that a Borrow will get invalid at a certain point. (Due to some operation, that is done at this point.)
```Datalog
.decl invalidates(B:Borrow, P:Point)
.input invalidates
```

