{ parity probe: v-slot-53 }
unit P_v_slot_53;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
  c: Cardinal;
begin
  c := 5;
  v := VarType(c);
  Show('VarType(c)', v);
end;
end.
