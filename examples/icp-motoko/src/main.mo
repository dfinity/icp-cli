import Text "mo:base/Text";

actor {
  public query func greet(name : Text) : async Text {
    return "Hello, " # name # "!";
  };
};
