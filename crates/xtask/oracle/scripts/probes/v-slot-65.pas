{ parity probe: v-slot-65 }
unit P_v_slot_65;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
  i: Integer;
begin
  v := i;
  Show('i', v);
end;
end.
