{ parity probe: x-071 }
unit P_x_071;

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
  v := StrToInt('$FFFFFFFF');
  AddMessage('after');
end;
end.
