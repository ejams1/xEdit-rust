{ parity probe: v-slot-64 }
unit P_v_slot_64;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := VarType(ElementCount(ElementByIndex(FileByIndex(0), 0)));
  Show('VarType(ElementCount(ElementByIndex(FileByIndex(0), 0)))', v);
end;
end.
