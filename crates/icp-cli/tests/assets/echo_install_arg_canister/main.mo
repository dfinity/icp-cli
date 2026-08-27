import Nat8 "mo:base/Nat8";

// Reports the argument of the most recent install *or upgrade*: `lastArg` is
// transient, so the class body re-runs with the new argument on every upgrade
// instead of the value being restored from the old heap.
persistent actor class EchoInstallArg(arg : ?Nat8) {
    transient let lastArg : ?Nat8 = arg;

    public query func get() : async Text {
        switch (lastArg) {
            case (null) { "no arg" };
            case (?number) { Nat8.toText(number) };
        };
    };
};
