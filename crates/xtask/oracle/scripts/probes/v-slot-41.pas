{ parity probe: v-slot-41 }
unit P_v_slot_41;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
  lst: TList;
begin
  lst := TList.Create;
  v := VarType(lst.Count);
  Show('VarType(lst.Count)', v);
end;
end.
