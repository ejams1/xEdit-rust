{ parity probe: v-slot-58 }
unit P_v_slot_58;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := VarType(TList.Create.Count);
  Show('VarType(TList.Create.Count)', v);
end;
end.
