{ parity probe: v-slot-38 }
unit P_v_slot_38;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := VarType(StrToInt('5'));
  Show('VarType(StrToInt(''5''))', v);
end;
end.
