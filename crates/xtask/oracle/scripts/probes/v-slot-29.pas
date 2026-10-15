{ parity probe: v-slot-29 }
unit P_v_slot_29;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := VarType(IntToStr(5));
  Show('VarType(IntToStr(5))', v);
end;
end.
