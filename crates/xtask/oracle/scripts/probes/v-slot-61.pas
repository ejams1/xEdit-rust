{ parity probe: v-slot-61 }
unit P_v_slot_61;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := VarType(StrToIntDef('x', 7));
  Show('VarType(StrToIntDef(''x'', 7))', v);
end;
end.
