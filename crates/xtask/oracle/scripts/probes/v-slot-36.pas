{ parity probe: v-slot-36 }
unit P_v_slot_36;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := VarType(GetLoadOrderFormID(FileByIndex(0)));
  Show('VarType(GetLoadOrderFormID(FileByIndex(0)))', v);
end;
end.
