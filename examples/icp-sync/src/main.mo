actor {
    // Variable to hold the number
    var number : Nat = 0;

    // Update method to set the number
    public func set(n : Nat) : async () {
        number := n;
    };

    // Query method to get the current number
    public query func get() : async Nat {
        return number;
    };
};
