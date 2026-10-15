{ parity probe: v-slot-60 }
unit P_v_slot_60;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := VarType(Ord(True));
  Show('VarType(Ord(True))', v);
end;
end.
