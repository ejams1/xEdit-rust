{ parity probe: x-269 }
unit P_x_269;

function Initialize: integer;
var
  i: Integer;
  d: Double;
  s: string;
  b: Boolean;
  sl: TStringList;
  v: Variant;
begin
  sl := TStringList.Create;
  sl.Add('a');
  v := StrToInt(True);
  AddMessage('after');
end;
end.
