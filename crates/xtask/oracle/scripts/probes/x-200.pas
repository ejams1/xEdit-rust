{ parity probe: x-200 }
unit P_x_200;

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
  v := -nil;
  AddMessage('after');
end;
end.
