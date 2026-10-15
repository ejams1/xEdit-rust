{ parity probe: v-slot-30 }
unit P_v_slot_30;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := VarType(1.5);
  Show('VarType(1.5)', v);
end;
end.
