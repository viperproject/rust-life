# Naive polonius borrowchecker rules, v0.8.0
There follows a list of all rules that are used by the polonius borrow checker (naive version), that were found in the file [polonius/polonius-engine/src/output/naive.rs](https://github.com/rust-lang/polonius/blob/master/polonius-engine/src/output/naive.rs). The rules are given as Datalog rules (using Soufflé project syntax), basically they are just copies of the comments in source file. In addition, we also try to give short explanations of all rules, these explanations are mostly based on the information in the blog post [An alias-based formulation of the borrow checker](http://smallcultfollowing.com/babysteps/blog/2018/04/27/an-alias-based-formulation-of-the-borrow-checker/) by Nicholas Matsakis, i.e. basically the first introduction of the polonius borrow checker.

This document gives the rules from version 0.4.0 of polonius, i.e. the ones from polonius-engine version 0.8.0.

Note that the construct that previously was called "loan", and often denoted as 'L' seems to now be called "borrow", and is now denoted as 'B'.

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

