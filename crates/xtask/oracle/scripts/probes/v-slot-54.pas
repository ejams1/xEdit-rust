{ parity probe: v-slot-54 }
unit P_v_slot_54;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
  w: Word;
begin
  w := 5;
  v := VarType(w);
  Show('VarType(w)', v);
end;
end.
