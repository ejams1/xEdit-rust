{ parity probe: v-slot-40 }
unit P_v_slot_40;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
  sl: TStringList;
begin
  sl := TStringList.Create;
  v := VarType(sl.Count);
  Show('VarType(sl.Count)', v);
end;
end.
