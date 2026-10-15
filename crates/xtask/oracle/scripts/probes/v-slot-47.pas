{ parity probe: v-slot-47 }
unit P_v_slot_47;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := VarType(StrToInt64('5'));
  Show('VarType(StrToInt64(''5''))', v);
end;
end.
