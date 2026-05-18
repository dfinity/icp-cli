import Debug "mo:core/Debug";
import Timer "mo:core/Timer";

persistent actor {

  // Simple log: prints the submitted text once
  public func log(t : Text) : async () {
    Debug.print(t);
  };

  // Helper function that logs "i text" and cancels the timer after 5 times
  private func startRepeatedLog(t : Text) : async () {
    var i : Nat = 0;
    var timerId : ?Timer.TimerId = null;

    // This job will be called every second
    func job() : async () {
      i += 1;
      Debug.print(debug_show (i) # " " # t);

      if (i == 5) {
        // Cancel the recurring timer after 5 executions
        switch (timerId) {
          case (?id) { Timer.cancelTimer(id) };
          case null {};
        };
      };
    };

    // Start a recurring timer every 1 second
    let id = Timer.recurringTimer<system>(#seconds 1, job);
    timerId := ?id;
  };

  // Public function: sets up the repeated logging
  public func log_repeated(t : Text) : async () {
    await startRepeatedLog(t);
  };
};
