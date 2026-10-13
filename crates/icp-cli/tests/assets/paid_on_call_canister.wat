;; A stand-in for CyclePay's backend canister, answering `order_status` (query)
;; for every order id, Candid-encoded as the real canister sends it (every
;; field of the record, in its order): order `co-1-0000test`, $10.00, unpaid
;; and payable until 2100-01-01T00:00:00Z until the update `pay` is called,
;; and from then on paid and delivered, 6.938T cycles to canister
;; rrkah-fqaaa-aaaaa-aaaaq-cai.
;;
;; Rebuild with: wasm-tools parse paid_on_call_canister.wat -o paid_on_call_canister.wasm
(module
  (import "ic0" "msg_reply_data_append" (func $reply_data_append (param i32 i32)))
  (import "ic0" "msg_reply" (func $reply))
  (memory 1)
  ;; Unpaid at 0, delivered at 1024, an empty Candid reply at 2048. The
  ;; word at 4096 is 1 once `pay` has been called.
  (data (i32.const 0) "\44\49\44\4c\06\6e\01\6c\10\b5\d3\a0\27\02\b2\ce\ef\2f\03\92\c7\e9\57\04\8e\95\ee\8f\01\05\e4\a8\c4\c3\02\02\b3\b0\da\c3\03\68\f0\8c\c4\8d\04\78\aa\ac\d9\d0\06\78\91\c5\fd\80\07\02\b1\ad\cb\e1\09\78\86\bd\da\8b\0b\04\ac\9c\e7\81\0c\71\c1\d9\8f\a4\0c\02\ab\9f\e4\d9\0c\78\de\a7\f7\da\0d\04\df\c5\d8\de\0d\04\6e\71\6b\08\ca\c6\99\a0\01\7f\f7\8d\ab\cb\02\7f\e4\80\bf\9f\05\7f\fa\e9\b0\eb\05\7f\bf\88\9f\83\06\7f\91\ec\f7\db\09\7f\b5\e5\a2\fa\09\7f\f3\bb\99\ef\0c\7f\6e\78\6b\02\c7\bc\99\b4\07\68\c9\b3\ff\e7\0c\68\01\00\01\00\03\00\00\01\0a\00\00\00\00\00\00\00\01\01\01\00\01\0a\00\00\00\00\00\00\00\01\01\01\ad\03\00\00\00\00\00\00\00\80\c9\6a\00\00\00\00\00\00\00\00\00\00\00\00\00\00\0d\63\6f\2d\31\2d\30\30\30\30\74\65\73\74\00\e8\03\00\00\00\00\00\00\01\00\57\86\f4\00\00\00\00\00")
  (data (i32.const 1024) "\44\49\44\4c\06\6e\01\6c\10\b5\d3\a0\27\02\b2\ce\ef\2f\03\92\c7\e9\57\04\8e\95\ee\8f\01\05\e4\a8\c4\c3\02\02\b3\b0\da\c3\03\68\f0\8c\c4\8d\04\78\aa\ac\d9\d0\06\78\91\c5\fd\80\07\02\b1\ad\cb\e1\09\78\86\bd\da\8b\0b\04\ac\9c\e7\81\0c\71\c1\d9\8f\a4\0c\02\ab\9f\e4\d9\0c\78\de\a7\f7\da\0d\04\df\c5\d8\de\0d\04\6e\71\6b\08\ca\c6\99\a0\01\7f\f7\8d\ab\cb\02\7f\e4\80\bf\9f\05\7f\fa\e9\b0\eb\05\7f\bf\88\9f\83\06\7f\91\ec\f7\db\09\7f\b5\e5\a2\fa\09\7f\f3\bb\99\ef\0c\7f\6e\78\6b\02\c7\bc\99\b4\07\68\c9\b3\ff\e7\0c\68\01\00\01\01\07\70\69\5f\74\65\73\74\02\01\d4\ca\1a\7f\4f\06\00\00\00\01\0a\00\00\00\00\00\00\00\01\01\01\01\07\63\73\5f\74\65\73\74\01\0a\00\00\00\00\00\00\00\01\01\01\ad\03\00\00\00\00\00\00\00\80\c9\6a\00\00\00\00\00\00\00\00\00\00\00\00\00\01\3c\80\c9\6a\00\00\00\00\0d\63\6f\2d\31\2d\30\30\30\30\74\65\73\74\00\e8\03\00\00\00\00\00\00\00\01\e8\03\00\00\00\00\00\00")
  (data (i32.const 2048) "DIDL\00\00")
  (func $order_status
    (if (i32.load (i32.const 4096))
      (then (call $reply_data_append (i32.const 1024) (i32.const 293)))
      (else (call $reply_data_append (i32.const 0) (i32.const 261))))
    (call $reply))
  (func $pay
    (i32.store (i32.const 4096) (i32.const 1))
    (call $reply_data_append (i32.const 2048) (i32.const 6))
    (call $reply))
  (export "canister_query order_status" (func $order_status))
  (export "canister_update pay" (func $pay)))
