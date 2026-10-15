{ parity probe: v-slot-42 }
unit P_v_slot_42;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := VarType(Pred(3));
  Show('VarType(Pred(3))', v);
end;
end.
