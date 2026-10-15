{ parity probe: v-slot-19 }
unit P_v_slot_19;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
  i: Integer;
begin
  i := '';
  v := i;
  Show('i', v);
end;
end.
