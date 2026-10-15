{ parity probe: v-slot-79 }
unit P_v_slot_79;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
  v: Variant;
begin
  v := VarType(v);
  Show('VarType(v)', v);
end;
end.
