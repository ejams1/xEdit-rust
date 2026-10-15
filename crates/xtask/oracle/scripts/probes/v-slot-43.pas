{ parity probe: v-slot-43 }
unit P_v_slot_43;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := VarType(ElementCount(FileByIndex(0)));
  Show('VarType(ElementCount(FileByIndex(0)))', v);
end;
end.
