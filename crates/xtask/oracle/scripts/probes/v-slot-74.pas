{ parity probe: v-slot-74 }
unit P_v_slot_74;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
  s: string;
begin
  s := 'abc';
  v := VarType(s[1]);
  Show('VarType(s[1])', v);
end;
end.
