{ parity probe: v-slot-67 }
unit P_v_slot_67;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := VarType([1, 2]);
  Show('VarType([1, 2])', v);
end;
end.
