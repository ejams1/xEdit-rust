{ parity probe: v-slot-37 }
unit P_v_slot_37;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := VarType(Ord('a'));
  Show('VarType(Ord(''a''))', v);
end;
end.
