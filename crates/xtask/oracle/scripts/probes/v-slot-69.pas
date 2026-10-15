{ parity probe: v-slot-69 }
unit P_v_slot_69;

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
  v := Ord(c);
  Show('Ord(c)', v);
end;
end.
