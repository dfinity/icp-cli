import Nat8 "mo:base/Nat8";

persistent actor class EchoInitArg(initNumber : ?Nat8) {
    var storedNumber : ?Nat8 = initNumber;

    public query func get() : async Text {
        switch (storedNumber) {
            case (null) { "no init" };
            case (?number) { Nat8.toText(number) };
        };
    };
};
