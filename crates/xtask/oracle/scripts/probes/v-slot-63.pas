{ parity probe: v-slot-63 }
unit P_v_slot_63;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := VarType(wbVersionNumber);
  Show('VarType(wbVersionNumber)', v);
end;
end.
