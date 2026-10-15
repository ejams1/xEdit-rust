{ parity probe: v-slot-70 }
unit P_v_slot_70;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
  c: Char;
begin
  c := 'a';
  v := VarType(c);
  Show('VarType(c)', v);
end;
end.
