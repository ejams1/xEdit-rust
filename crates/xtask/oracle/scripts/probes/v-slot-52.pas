{ parity probe: v-slot-52 }
unit P_v_slot_52;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
  i: Int64;
begin
  i := 5;
  v := VarType(i);
  Show('VarType(i)', v);
end;
end.
